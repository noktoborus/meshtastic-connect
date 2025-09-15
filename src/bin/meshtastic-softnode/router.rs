use std::sync::Arc;

use meshtastic_connect::transport::mqtt;
use tokio::{sync::Mutex, task::JoinSet};

pub type ConnectionName = String;
type ConnectionId = usize;
use crate::{
    config::{TransportQuirk, TransportQuirks},
    connection,
};

impl Router {
    pub fn add_connection(
        &mut self,
        connection_name: String,
        quirks: TransportQuirks,
        connection: (
            connection::Sender,
            connection::Receiver,
            Option<connection::Heartbeat>,
        ),
    ) {
        let (send, recv, interruptor) = connection;
        let id = self.connections.len();

        println!("Wait data for {} [{}]", connection_name, id);
        self.connections.push(ConnectionCapsule {
            id,
            name: connection_name,
            quirks,
            send: Arc::new(Mutex::new(send)),
        });

        set_wait_data(&mut self.recv_set, recv, id);
        if let Some(interruptor) = interruptor {
            set_wait_interrupt(&mut self.interrupt_set, interruptor, id);
        }
    }

    // Send a mesh packet to all connections except the one specified by `from`
    async fn send_mesh_except(
        &mut self,
        channel: Option<mqtt::ChannelId>,
        mesh_packet: &meshtastic_connect::meshtastic::MeshPacket,
        source_connection_id: Option<ConnectionId>,
    ) {
        for capsule in self.connections.iter_mut() {
            if let Some(source_connection_id) = source_connection_id {
                if capsule.id == source_connection_id {
                    continue;
                }
            }
            println!("> {:?} send: {:?}", capsule.name, mesh_packet);
            let mut mesh_packet = mesh_packet.clone();
            let channel = channel.clone();

            apply_quirk_to_packet(&mut mesh_packet, &capsule.quirks.output);

            let send = capsule.send.clone();
            tokio::spawn(async move { send.lock().await.send((channel, mesh_packet)).await });
        }
    }
}

struct ConnectionCapsule {
    id: ConnectionId,
    name: ConnectionName,
    quirks: TransportQuirks,
    send: Arc<Mutex<connection::Sender>>,
}

pub struct ReceiveCapsule {
    pub source_connection_name: ConnectionName,
    pub source_connection_id: ConnectionId,
    pub incoming: connection::Incoming,
}

type RecvSet =
    JoinSet<Result<(ConnectionId, connection::Incoming, connection::Receiver), std::io::Error>>;

type InterruptSet = JoinSet<(ConnectionId, connection::Heartbeat)>;

#[derive(Default)]
pub struct Router {
    connections: Vec<ConnectionCapsule>,

    // Receiving set
    recv_set: RecvSet,

    // Interrupting set
    interrupt_set: InterruptSet,
}

impl Router {
    // Send to all connections
    pub async fn send_mesh(
        &mut self,
        channel: Option<mqtt::ChannelId>,
        mesh_packet: meshtastic_connect::meshtastic::MeshPacket,
    ) {
        self.send_mesh_except(channel, &mesh_packet, None).await;
    }

    // Send to next transports' endpoint
    pub async fn route_next(
        &mut self,
        channel: Option<mqtt::ChannelId>,
        recv_capsule: ReceiveCapsule,
    ) {
        if let connection::DataVariant::MeshPacket(ref mesh_packet) = recv_capsule.incoming.data {
            self.send_mesh_except(
                channel.or(recv_capsule.incoming.channel_id),
                &mesh_packet,
                Some(recv_capsule.source_connection_id),
            )
            .await;
        }
    }

    // Try to receive from all connections and send to all, except received
    pub async fn recv_mesh(&mut self) -> Result<ReceiveCapsule, std::io::Error> {
        loop {
            if self.interrupt_set.is_empty() {
                if let Some(res) = self.recv_set.join_next().await {
                    return self.process_join_recv(res).await;
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("No connections available"),
                    ));
                }
            } else {
                tokio::select! {
                    Some(res) = self.interrupt_set.join_next() => {
                        self.process_join_interrupt(res).await?
                    }
                    res = self.recv_set.join_next()  => {
                        if let Some(res) = res {
                        return self.process_join_recv(res).await;
                        }
                        else {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("No connections available"),
                            ));
                        }
                    }
                }
            }
        }
    }

    async fn process_join_interrupt(
        &mut self,
        res: Result<(ConnectionId, connection::Heartbeat), tokio::task::JoinError>,
    ) -> Result<(), std::io::Error> {
        let res = res.map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("interrupt join error: {}", e),
            )
        })?;

        let (capsule_id, interruptor) = res;
        let mut sender = self.connections[capsule_id].send.lock().await;

        interruptor.send(&mut sender).await?;

        set_wait_interrupt(&mut self.interrupt_set, interruptor, capsule_id);
        Ok(())
    }

    async fn process_join_recv(
        &mut self,
        res: Result<
            Result<(ConnectionId, connection::Incoming, connection::Receiver), std::io::Error>,
            tokio::task::JoinError,
        >,
    ) -> Result<ReceiveCapsule, std::io::Error> {
        let res = res.map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("receiving join error: {}", e),
            )
        })?;

        let (capsule_id, mut incoming, recv) = res?;
        let capsule = &self.connections[capsule_id];

        if let connection::DataVariant::MeshPacket(ref mut mesh_packet) = incoming.data {
            println!("> {:?} received: {:?}", capsule.name, mesh_packet);
            apply_quirk_to_packet(mesh_packet, &capsule.quirks.input);
        }

        set_wait_data(&mut self.recv_set, recv, capsule_id);

        return Ok(ReceiveCapsule {
            source_connection_name: capsule.name.clone(),
            source_connection_id: capsule.id,
            incoming,
        });
    }
}

fn set_wait_data(recv_set: &mut RecvSet, mut recv: connection::Receiver, id: ConnectionId) {
    recv_set.spawn(async move { recv.next().await.map(|r| (id, r, recv)) });
}

fn set_wait_interrupt(
    interrupt_set: &mut InterruptSet,
    mut interrupt: connection::Heartbeat,
    id: ConnectionId,
) {
    interrupt_set.spawn(async move {
        interrupt.next().await;
        (id, interrupt)
    });
}

fn apply_quirk_to_packet(
    mesh_packet: &mut meshtastic_connect::meshtastic::MeshPacket,
    quirks: &Vec<TransportQuirk>,
) {
    const HOP_LIMIT_MAX: u32 = 7;
    for quirk in quirks {
        match quirk {
            TransportQuirk::IncrementHopLimit => {
                if mesh_packet.hop_limit < HOP_LIMIT_MAX {
                    mesh_packet.hop_limit += 1
                }
            }
            TransportQuirk::SetViaMQTT => mesh_packet.via_mqtt = true,
            TransportQuirk::UnsetViaMQTT => mesh_packet.via_mqtt = false,
            TransportQuirk::FixupHopStartIf0 => {
                if mesh_packet.hop_start == 0 && mesh_packet.hop_limit == HOP_LIMIT_MAX {
                    mesh_packet.hop_start = mesh_packet.hop_limit;
                }
            }
        }
    }
}
