use crate::{datachannel, protocol};

pub struct Transport {
    dc: std::sync::Arc<datachannel::DataChannel>,
}

impl Transport {
    pub fn new(dc: std::sync::Arc<datachannel::DataChannel>) -> Self {
        Self { dc }
    }

    pub async fn send_init(
        &self,
        battle_number: u8,
        input_delay: u32,
        marshaled: &[u8],
    ) -> anyhow::Result<()> {
        self.dc
            .send(
                protocol::Packet::Init(protocol::Init {
                    battle_number,
                    input_delay,
                    marshaled: marshaled.to_vec(),
                })
                .serialize()?
                .as_slice(),
            )
            .await?;
        Ok(())
    }

    pub async fn send_input(
        &self,
        battle_number: u8,
        local_tick: u32,
        remote_tick: u32,
        joyflags: u16,
        custom_screen_state: u8,
        turn: Vec<u8>,
    ) -> anyhow::Result<()> {
        self.dc
            .send(
                protocol::Packet::Input(protocol::Input {
                    battle_number,
                    local_tick,
                    remote_tick,
                    joyflags,
                    custom_screen_state,
                    turn,
                })
                .serialize()?
                .as_slice(),
            )
            .await?;
        Ok(())
    }
}
