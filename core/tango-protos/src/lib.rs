pub mod signaling {
    include!(concat!(env!("OUT_DIR"), "/tango.signaling.rs"));
}

pub mod ipc {
    include!(concat!(env!("OUT_DIR"), "/tango.ipc.rs"));
}
