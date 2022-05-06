pub mod signaling {
    include!(concat!(env!("OUT_DIR"), "/tango.signaling.rs"));
}

pub mod lobby {
    include!(concat!(env!("OUT_DIR"), "/tango.lobby.rs"));
}
