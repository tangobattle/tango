//! The export session: frames and samples in, container bytes out.
//!
//! One session type serves every backend. It owns the muxer and does the
//! one job no encoder can: deciding *when* a packet is safe to write.
//! Video and audio are encoded independently and finish at their own
//! pace, so packets arrive interleaved arbitrarily while a container
//! wants them in time order. Each track therefore gets a queue, and a
//! packet is released only once every other track has produced something
//! at least as late — the earliest packet nothing still to come can
//! precede.
//!
//! Like the muxers, a session does no I/O. Finished bytes come out of
//! [`Session::take_output`] for the caller to append however it writes
//! files — synchronously to a [`std::fs::File`], or awaited into a
//! browser's file stream — and [`Session::poll_finish`] hands back the
//! last bytes plus the [`Fixup`]s that complete the parts written
//! earlier. Closing consumes the muxer, so it happens exactly once.

use std::collections::VecDeque;

use crate::backend::{Backend, PlatformBackend, VIDEO_TRACK};
use crate::error::check;
use crate::mux::{self, Chapter, Fixup, MuxConfig, Muxer};
use crate::packet::ticks_to_ns;
use crate::{AudioTrackInfo, Canceller, Packet, Settings, VideoTrackInfo};

pub struct Session<B: Backend = PlatformBackend> {
    backend: B,
    settings: Settings,
    /// Set once every track has revealed its codec configuration.
    muxer: Option<Box<dyn Muxer>>,
    /// Queue per track: video first, then audio in order.
    queues: Vec<VecDeque<Packet>>,
    timescales: Vec<u32>,
    output: Vec<u8>,
    video_frames: u64,
    flushing: bool,
    finished: bool,
}

impl Session<PlatformBackend> {
    /// Open an export: check the settings, then start this platform's
    /// encoders on them.
    ///
    /// Which encoder that is isn't a choice — there is one per target
    /// (see [`PlatformBackend`]), so a caller that would only ever have
    /// written `#[cfg]` to pick shouldn't have to.
    pub fn new(settings: Settings, canceller: &Canceller) -> crate::Result<Self> {
        settings.validate()?;
        canceller.check()?;
        let backend = PlatformBackend::open(&settings, canceller)?;
        Self::with_backend(backend, settings)
    }
}

impl<B: Backend> Session<B> {
    /// Open an export on a backend of your own — a test double, or an
    /// encoder this crate doesn't know about. [`Session::new`] is the
    /// one to reach for otherwise.
    pub fn with_backend(backend: B, settings: Settings) -> crate::Result<Self> {
        settings.validate()?;
        let mut timescales = vec![settings.video.timescale];
        timescales.extend(std::iter::repeat_n(settings.audio.sample_rate, settings.audio_tracks));
        Ok(Self {
            backend,
            muxer: None,
            queues: (0..=settings.audio_tracks).map(|_| VecDeque::new()).collect(),
            timescales,
            output: Vec::new(),
            video_frames: 0,
            flushing: false,
            finished: false,
            settings,
        })
    }

    /// Push one RGBA frame at the input size.
    pub fn write_video(&mut self, rgba: &[u8]) -> crate::Result<()> {
        let expected = self.settings.video.frame_bytes();
        check!(
            rgba.len() == expected,
            "expected a {expected}-byte RGBA frame, got {}",
            rgba.len()
        );
        self.backend.submit_video(rgba)?;
        self.video_frames += 1;
        self.pump(false)
    }

    /// Push interleaved samples for one audio track.
    pub fn write_audio(&mut self, track: usize, samples: &[i16]) -> crate::Result<()> {
        check!(track < self.settings.audio_tracks, "no audio track {track}");
        let channels = self.settings.audio.channels as usize;
        check!(
            channels > 0 && samples.len().is_multiple_of(channels),
            "{} samples don't divide into {channels} channels",
            samples.len()
        );
        self.backend.submit_audio(track, samples)?;
        self.pump(false)
    }

    /// Container bytes to append to the output. Call after every write,
    /// or memory grows with the length of the export instead of staying
    /// flat.
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    /// Video frames pushed so far.
    pub fn video_frames(&self) -> u64 {
        self.video_frames
    }

    /// The backend's unfinished queue — see [`Backend::queue_depth`].
    /// An export that can't be blocked pushes until this is deep, then
    /// goes away and comes back.
    pub fn queue_depth(&self) -> u32 {
        self.backend.queue_depth()
    }

    /// Ask the encoders to finish. Then call [`Session::poll_finish`]
    /// until it yields the fixups.
    pub fn begin_finish(&mut self) -> crate::Result<()> {
        self.flushing = true;
        self.backend.begin_flush()
    }

    /// Drive the shutdown. Returns `None` while the encoders are still
    /// working, or the fixups to apply once the container is closed.
    ///
    /// `chapters` are in output video frames.
    pub fn poll_finish(&mut self, chapters: &[Chapter]) -> crate::Result<Option<Vec<Fixup>>> {
        check!(self.flushing, "begin_finish must come first");
        check!(!self.finished, "this session has already been finished");
        let done = self.backend.poll_flush()?;
        // Collect whatever flushing produced before deciding anything:
        // the tail of a stream can arrive in the same step that reports
        // the encoders finished.
        self.pump(done)?;
        if !done {
            return Ok(None);
        }
        // Closing consumes the muxer, so nothing can be written to the
        // container after this and nothing can close it twice.
        let Some(muxer) = self.muxer.take() else {
            return Err(crate::Error::Empty);
        };
        self.finished = true;
        let closed = muxer.finish(chapters)?;
        self.output.extend_from_slice(&closed.bytes);
        Ok(Some(closed.fixups))
    }

    /// Collect what the encoders have produced and write out what's
    /// safely ordered.
    fn pump(&mut self, drain: bool) -> crate::Result<()> {
        for (track, packet) in self.backend.poll()? {
            let queue = self
                .queues
                .get_mut(track)
                .ok_or_else(|| {
                    crate::Error::internal(format!("the backend produced a packet for unknown track {track}"))
                })?;
            queue.push_back(packet);
        }
        self.open_muxer_if_ready()?;
        self.release(drain)
    }

    /// Write out every packet whose position in the timeline is settled.
    ///
    /// While a track's queue is empty its next packet could still turn
    /// out to be the earliest, so nothing after it can be written yet —
    /// unless `drain` says the streams are over.
    fn release(&mut self, drain: bool) -> crate::Result<()> {
        if self.muxer.is_none() {
            return Ok(());
        }
        loop {
            let mut earliest: Option<(usize, u64)> = None;
            let mut waiting = false;
            for (track, queue) in self.queues.iter().enumerate() {
                match queue.front() {
                    Some(packet) => {
                        let ns = ticks_to_ns(packet.pts, self.timescales[track]);
                        if earliest.is_none_or(|(_, best)| ns < best) {
                            earliest = Some((track, ns));
                        }
                    }
                    None => waiting = true,
                }
            }
            if waiting && !drain {
                break;
            }
            let Some((track, _)) = earliest else { break };
            let packet = self.queues[track].pop_front().expect("front was just read");
            // Track numbering is the muxers' too, so it passes straight
            // through.
            self.muxer.as_mut().expect("checked above").write(track, &packet)?;
        }
        let bytes = self.muxer.as_mut().expect("checked above").take_output();
        self.output.extend_from_slice(&bytes);
        Ok(())
    }

    /// Open the container once every track can describe itself.
    fn open_muxer_if_ready(&mut self) -> crate::Result<()> {
        // Not after the close: a write that arrived late must not start a
        // second container.
        if self.muxer.is_some() || self.finished {
            return Ok(());
        }
        let Some(video_private) = self.backend.codec_private(VIDEO_TRACK) else {
            return Ok(());
        };
        let mut audio = Vec::with_capacity(self.settings.audio_tracks);
        for track in 0..self.settings.audio_tracks {
            let Some(codec_private) = self.backend.codec_private(track + 1) else {
                return Ok(());
            };
            audio.push(AudioTrackInfo {
                codec: self.settings.audio.codec,
                sample_rate: self.settings.audio.sample_rate,
                channels: self.settings.audio.channels,
                codec_private,
                codec_delay_samples: self.backend.codec_delay_samples(track + 1),
            });
        }
        let video = &self.settings.video;
        self.muxer = Some(mux::open(MuxConfig {
            container: self.settings.container,
            video: VideoTrackInfo {
                codec: video.codec,
                width: video.output_width(),
                height: video.output_height(),
                timescale: video.timescale,
                frame_duration: video.frame_duration,
                color: video.color,
                codec_private: video_private,
            },
            audio,
            writing_app: concat!("encoder-facade ", env!("CARGO_PKG_VERSION")).to_string(),
            faststart: self.settings.faststart,
        })?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioCodec, AudioSettings, Container, H264Quality, VideoCodec, VideoSettings};

    /// A backend that hands back packets on demand, so the session's own
    /// behaviour — ordering, header timing, output draining — can be
    /// tested without an encoder anywhere near it.
    #[derive(Default)]
    struct FakeBackend {
        ready: Vec<(usize, Packet)>,
        video_private: Option<Vec<u8>>,
        audio_private: Option<Vec<u8>>,
        flushed: bool,
    }

    impl Backend for FakeBackend {
        fn submit_video(&mut self, _frame: &[u8]) -> crate::Result<()> {
            Ok(())
        }
        fn submit_audio(&mut self, _track: usize, _samples: &[i16]) -> crate::Result<()> {
            Ok(())
        }
        fn poll(&mut self) -> crate::Result<Vec<(usize, Packet)>> {
            Ok(std::mem::take(&mut self.ready))
        }
        fn codec_private(&self, track: usize) -> Option<Vec<u8>> {
            if track == VIDEO_TRACK {
                self.video_private.clone()
            } else {
                self.audio_private.clone()
            }
        }
        fn codec_delay_samples(&self, _track: usize) -> u64 {
            0
        }
        fn begin_flush(&mut self) -> crate::Result<()> {
            self.flushed = true;
            Ok(())
        }
        fn poll_flush(&mut self) -> crate::Result<bool> {
            Ok(self.flushed)
        }
    }

    fn settings() -> Settings {
        Settings {
            video: VideoSettings {
                codec: VideoCodec::H264 {
                    quality: H264Quality::Crf(18),
                },
                width: 240,
                height: 160,
                scale: 3,
                keyframe_interval: 120,
                timescale: 16_777_216,
                frame_duration: 280_896,
                color: None,
            },
            audio: AudioSettings {
                codec: AudioCodec::Aac { bitrate: 384_000 },
                sample_rate: 48_000,
                channels: 2,
            },
            container: Container::Matroska,
            audio_tracks: 1,
            faststart: false,
        }
    }

    fn packet(pts: u64, duration: u64) -> Packet {
        Packet {
            pts,
            duration,
            keyframe: true,
            data: vec![0u8; 8],
        }
    }

    /// A real avcC record, so the muxer can open its header.
    fn avcc() -> Vec<u8> {
        vec![
            1, 0x64, 0x00, 0x0d, 0xff, 0xe1, 0x00, 0x04, 0x67, 0x64, 0x00, 0x0d, 0x01, 0x00, 0x02, 0x68, 0xee,
        ]
    }

    #[test]
    fn nothing_is_written_until_every_track_can_describe_itself() {
        let mut session = Session::with_backend(FakeBackend::default(), settings()).unwrap();
        session.backend.ready.push((0, packet(0, 280_896)));
        session.write_video(&vec![0u8; 240 * 160 * 4]).unwrap();
        assert!(session.take_output().is_empty(), "no header without codec configuration");

        // The video encoder reveals its parameter sets, but the audio
        // encoder hasn't yet: still nothing.
        session.backend.video_private = Some(avcc());
        session.write_video(&vec![0u8; 240 * 160 * 4]).unwrap();
        assert!(session.take_output().is_empty(), "audio configuration is still missing");

        session.backend.audio_private = Some(vec![0x11, 0x90]);
        session.write_video(&vec![0u8; 240 * 160 * 4]).unwrap();
        let head = session.take_output();
        assert!(!head.is_empty(), "the header lands once both tracks are known");
        assert_eq!(&head[..4], &[0x1A, 0x45, 0xDF, 0xA3], "EBML head");
    }

    /// The point of the queues: a packet is only written once no
    /// still-missing packet from another track could come before it.
    #[test]
    fn a_packet_waits_for_the_other_track_to_catch_up() {
        let mut session = Session::with_backend(FakeBackend::default(), settings()).unwrap();
        session.backend.video_private = Some(avcc());
        session.backend.audio_private = Some(vec![0x11, 0x90]);
        // Drain the header first, so what's measured below is packet
        // bytes rather than the head of the file.
        session.backend.video_private = Some(avcc());
        session.write_video(&vec![0u8; 240 * 160 * 4]).unwrap();
        let _header = session.take_output();

        // Video for a full second arrives while audio has produced
        // nothing.
        for frame in 0..60u64 {
            session.backend.ready.push((0, packet(frame * 280_896, 280_896)));
        }
        session.write_video(&vec![0u8; 240 * 160 * 4]).unwrap();
        assert_eq!(
            session.take_output().len(),
            0,
            "no video can be written while audio might still turn up ahead of it"
        );

        // Audio arrives; now the video frames it can no longer precede
        // are safe to write.
        session.backend.ready.push((1, packet(24_000, 1024)));
        session.write_audio(0, &[0i16; 2]).unwrap();
        assert!(
            !session.take_output().is_empty(),
            "audio arriving unblocks the video before it"
        );
    }

    #[test]
    fn finish_drains_everything_and_reports_fixups() {
        let mut session = Session::with_backend(FakeBackend::default(), settings()).unwrap();
        session.backend.video_private = Some(avcc());
        session.backend.audio_private = Some(vec![0x11, 0x90]);
        let mut file = Vec::new();
        for frame in 0..10u64 {
            session.backend.ready.push((0, packet(frame * 280_896, 280_896)));
            session.backend.ready.push((1, packet(frame * 1024, 1024)));
            session.write_video(&vec![0u8; 240 * 160 * 4]).unwrap();
            file.extend_from_slice(&session.take_output());
        }
        session.begin_finish().unwrap();
        let fixups = loop {
            if let Some(fixups) = session.poll_finish(&[]).unwrap() {
                break fixups;
            }
        };
        file.extend_from_slice(&session.take_output());
        assert!(!fixups.is_empty(), "Matroska fixes up its segment size at least");
        let file = crate::mux::apply(file, &fixups);
        // The result has to satisfy a demuxer nobody here wrote.
        let mut mkv = matroska_demuxer::MatroskaFile::open(std::io::Cursor::new(file)).unwrap();
        let mut frames = 0;
        let mut frame = matroska_demuxer::Frame::default();
        while mkv.next_frame(&mut frame).unwrap() {
            frames += 1;
        }
        assert_eq!(frames, 20);
    }

    /// Closing is terminal: the muxer is consumed by it, so a second
    /// close has to be refused rather than write a second index.
    #[test]
    fn finishing_twice_is_refused() {
        let mut session = Session::with_backend(FakeBackend::default(), settings()).unwrap();
        session.backend.video_private = Some(avcc());
        session.backend.audio_private = Some(vec![0x11, 0x90]);
        session.backend.ready.push((0, packet(0, 280_896)));
        session.backend.ready.push((1, packet(0, 1024)));
        session.write_video(&vec![0u8; 240 * 160 * 4]).unwrap();
        session.begin_finish().unwrap();
        assert!(session.poll_finish(&[]).unwrap().is_some());
        assert!(session.poll_finish(&[]).is_err(), "a second close must be refused");
    }

    /// A write that arrives after the close must not start a second
    /// container.
    #[test]
    fn writing_after_the_close_adds_nothing() {
        let mut session = Session::with_backend(FakeBackend::default(), settings()).unwrap();
        session.backend.video_private = Some(avcc());
        session.backend.audio_private = Some(vec![0x11, 0x90]);
        session.write_video(&vec![0u8; 240 * 160 * 4]).unwrap();
        session.begin_finish().unwrap();
        session.poll_finish(&[]).unwrap().expect("closes");
        let _ = session.take_output();
        session.write_video(&vec![0u8; 240 * 160 * 4]).unwrap();
        assert!(
            session.take_output().is_empty(),
            "nothing more should reach the output after the close"
        );
    }

    #[test]
    fn poll_finish_before_begin_finish_is_refused() {
        let mut session = Session::with_backend(FakeBackend::default(), settings()).unwrap();
        assert!(session.poll_finish(&[]).is_err());
    }

    #[test]
    fn a_wrong_sized_frame_is_refused() {
        let mut session = Session::with_backend(FakeBackend::default(), settings()).unwrap();
        assert!(session.write_video(&[0u8; 16]).is_err());
    }
}
