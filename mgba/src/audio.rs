use std::pin::Pin;

pub struct AudioBuffer {
    inner: Pin<Box<mgba_sys::mAudioBuffer>>,
}

unsafe impl Send for AudioBuffer {}

impl AudioBuffer {
    pub fn new(capacity: usize, channels: u32) -> Self {
        let mut inner: Pin<Box<mgba_sys::mAudioBuffer>> = Box::pin(unsafe { std::mem::zeroed() });
        unsafe {
            mgba_sys::mAudioBufferInit(inner.as_mut().get_unchecked_mut(), capacity, channels);
        }
        AudioBuffer { inner }
    }

    pub fn available(&self) -> usize {
        unsafe { mgba_sys::mAudioBufferAvailable(self.inner.as_ref().get_ref()) }
    }

    pub fn read(&mut self, samples: &mut [i16], count: usize) -> usize {
        unsafe { mgba_sys::mAudioBufferRead(self.inner.as_mut().get_unchecked_mut(), samples.as_mut_ptr(), count) }
    }

    pub fn clear(&mut self) {
        unsafe { mgba_sys::mAudioBufferClear(self.inner.as_mut().get_unchecked_mut()) }
    }

    pub fn as_mut_ptr(&mut self) -> *mut mgba_sys::mAudioBuffer {
        unsafe { self.inner.as_mut().get_unchecked_mut() as *mut _ }
    }
}

impl Drop for AudioBuffer {
    fn drop(&mut self) {
        unsafe { mgba_sys::mAudioBufferDeinit(self.inner.as_mut().get_unchecked_mut()) }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct AudioBufferMutRef<'a> {
    pub(super) ptr: *mut mgba_sys::mAudioBuffer,
    pub(super) _lifetime: std::marker::PhantomData<&'a mut ()>,
}

impl<'a> AudioBufferMutRef<'a> {
    pub fn available(&self) -> usize {
        unsafe { mgba_sys::mAudioBufferAvailable(self.ptr) }
    }

    pub fn read(&mut self, samples: &mut [i16], count: usize) -> usize {
        unsafe { mgba_sys::mAudioBufferRead(self.ptr, samples.as_mut_ptr(), count) }
    }

    pub fn clear(&mut self) {
        unsafe { mgba_sys::mAudioBufferClear(self.ptr) }
    }

    pub fn as_mut_ptr(&mut self) -> *mut mgba_sys::mAudioBuffer {
        self.ptr
    }
}

pub struct AudioResampler {
    inner: Pin<Box<mgba_sys::mAudioResampler>>,
}

unsafe impl Send for AudioResampler {}

impl AudioResampler {
    pub fn new() -> Self {
        let mut inner: Pin<Box<mgba_sys::mAudioResampler>> = Box::pin(unsafe { std::mem::zeroed() });
        unsafe {
            mgba_sys::mAudioResamplerInit(
                inner.as_mut().get_unchecked_mut(),
                mgba_sys::mInterpolatorType_mINTERPOLATOR_SINC,
            );
        }
        AudioResampler { inner }
    }

    /// Sets the source buffer + its sample rate. The C layer stores
    /// the pointer for use by subsequent [`Self::process`] calls — the
    /// caller must ensure `source` stays live until either `process`
    /// runs or a new source is set.
    pub fn set_source(&mut self, source: &mut AudioBufferMutRef<'_>, rate: f64, consume: bool) {
        unsafe {
            mgba_sys::mAudioResamplerSetSource(self.inner.as_mut().get_unchecked_mut(), source.ptr, rate, consume);
        }
    }

    /// Sets the destination buffer + its sample rate. As with
    /// [`Self::set_source`], the C layer stores the pointer across
    /// calls — the destination must outlive any later `process`.
    pub fn set_destination(&mut self, destination: &mut AudioBuffer, rate: f64) {
        unsafe {
            mgba_sys::mAudioResamplerSetDestination(
                self.inner.as_mut().get_unchecked_mut(),
                destination.inner.as_mut().get_unchecked_mut(),
                rate,
            );
        }
    }

    pub fn process(&mut self) -> usize {
        unsafe { mgba_sys::mAudioResamplerProcess(self.inner.as_mut().get_unchecked_mut()) }
    }
}

impl Default for AudioResampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioResampler {
    fn drop(&mut self) {
        unsafe { mgba_sys::mAudioResamplerDeinit(self.inner.as_mut().get_unchecked_mut()) }
    }
}
