use crate::protocol;

pub struct Transport {
    dc_tx: datachannel_wrapper::DataChannelSender,
    rendezvous_rx: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl Transport {
    pub fn new(
        dc_tx: datachannel_wrapper::DataChannelSender,
        rendezvous_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Transport {
        Transport {
            dc_tx,
            rendezvous_rx: Some(rendezvous_rx),
        }
    }

    pub async fn send_input(
        &mut self,
        round_number: u8,
        local_tick: u32,
        tick_diff: i8,
        joyflags: u16,
    ) -> anyhow::Result<()> {
        self.dc_tx
            .send(
                protocol::Packet::Input(protocol::Input {
                    round_number,
                    local_tick,
                    tick_diff,
                    joyflags,
                })
                .serialize()?
                .as_slice(),
            )
            .await?;
        if let Some(rendezvous_rx) = self.rendezvous_rx.take() {
            rendezvous_rx.await?;
        }
        Ok(())
    }
}
