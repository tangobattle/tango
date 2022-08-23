pub mod signaling {
    include!(concat!(env!("OUT_DIR"), "/tango.signaling.rs"));
}

pub mod iceconfig {
    include!(concat!(env!("OUT_DIR"), "/tango.iceconfig.rs"));
}

pub mod replay {
    include!(concat!(env!("OUT_DIR"), "/tango.replay.rs"));
}
