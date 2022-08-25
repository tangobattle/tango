pub mod matchmaking {
    include!(concat!(env!("OUT_DIR"), "/tango.matchmaking.rs"));
}

pub mod replay {
    include!(concat!(env!("OUT_DIR"), "/tango.replay.rs"));
}
