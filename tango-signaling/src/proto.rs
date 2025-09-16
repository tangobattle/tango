pub mod signaling {
    include!(concat!(env!("OUT_DIR"), "/tango.signaling.rs"));
}

pub use prost;
