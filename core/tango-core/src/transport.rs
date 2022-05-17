use crate::protocol;

pub struct Transport {
    dc_tx: datachannel_wrapper::DataChannelSender,
}

impl Transport {
    pub fn new(dc_tx: datachannel_wrapper::DataChannelSender) -> Transport {
        Transport { dc_tx }
    }

    pub async fn send_input(
        &mut self,
        round_number: u8,
        local_tick: u32,
        remote_tick: u32,
        joyflags: u16,
    ) -> anyhow::Result<()> {
        self.dc_tx
            .send(
                protocol::Packet::Input(protocol::Input {
                    round_number,
                    local_tick,
                    remote_tick,
                    joyflags,
                })
                .serialize()?
                .as_slice(),
            )
            .await?;
        Ok(())
    }

    pub async fn send_pong(&mut self, ts: u64) -> anyhow::Result<()> {
        self.dc_tx
            .send(
                protocol::Packet::Pong(protocol::Pong { ts })
                    .serialize()?
                    .as_slice(),
            )
            .await?;
        Ok(())
    }
}
