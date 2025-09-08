use meshtastic_connect::transport::mqtt;
use std::sync::Arc;
use tokio::{sync::Mutex, task::JoinSet};

pub type ConnectionName = String;
type ArcConnectionCapsule = Arc<ConnectionCapsule>;
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
        connection: connection::Connection,
    ) {
        self.connections.push(Arc::new(ConnectionCapsule {
            id: self.connections.len(),
            name: connection_name,
            quirks,
            connection: Mutex::new(connection),
        }));
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
            let capsule = capsule.clone();
            println!("> {:?} send: {:?}", capsule.name, mesh_packet);
            let mut mesh_packet = mesh_packet.clone();
            let channel = channel.clone();

            apply_quirk_to_packet(&mut mesh_packet, &capsule.quirks.output);
            tokio::spawn(async move {
                capsule
                    .connection
                    .lock()
                    .await
                    .send_mesh(channel, mesh_packet)
                    .await
            });
        }
    }
}

struct ConnectionCapsule {
    id: ConnectionId,
    name: ConnectionName,
    quirks: TransportQuirks,
    connection: Mutex<connection::Connection>,
}

pub struct ReceiveCapsule {
    pub source_connection_name: ConnectionName,
    pub source_connection_id: ConnectionId,
    pub incoming: connection::Incoming,
}

type RecvSet = JoinSet<Result<(ArcConnectionCapsule, connection::Incoming), std::io::Error>>;

#[derive(Default)]
pub struct Router {
    connections: Vec<ArcConnectionCapsule>,
    recv_set: RecvSet,
}

impl Router {
    /// Connect all connections
    pub async fn connect(&mut self) -> Result<(), std::io::Error> {
        let mut set = JoinSet::new();

        for capsule in &mut self.connections {
            let capsule = capsule.clone();
            set.spawn(async move { capsule.connection.lock().await.connect().await });
        }

        while let Some(res) = set.join_next().await {
            match res.map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, format!("thread panicked: {}", e))
            })? {
                Ok(_) => {}
                Err(e) => return Err(e),
            }
        }

        for capsule in self.connections.iter_mut() {
            add_connection_to_set(&mut self.recv_set, &capsule);
        }

        Ok(())
    }

    /// Disconnect all connections
    pub async fn disconnect(&mut self) {
        for capsule in &mut self.connections {
            let capsule = capsule.clone();
            tokio::spawn(async move { capsule.connection.lock().await.disconnect().await });
        }
    }

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
        while let Some(res) = self.recv_set.join_next().await {
            let (capsule, mut incoming) = res.map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, format!("thread panicked: {}", e))
            })??;

            if let connection::DataVariant::MeshPacket(ref mut mesh_packet) = incoming.data {
                println!("> {:?} received: {:?}", capsule.name, mesh_packet);
                apply_quirk_to_packet(mesh_packet, &capsule.quirks.input);
            }

            add_connection_to_set(&mut self.recv_set, &capsule);

            return Ok(ReceiveCapsule {
                source_connection_name: capsule.name.clone(),
                source_connection_id: capsule.id,
                incoming,
            });
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("No connections available"),
        ))
    }
}

fn add_connection_to_set(recv_set: &mut RecvSet, capsule: &ArcConnectionCapsule) {
    let capsule = capsule.clone();
    recv_set.spawn(async move {
        capsule
            .connection
            .lock()
            .await
            .recv_mesh()
            .await
            .map(|r| (capsule.clone(), r))
    });
}

fn apply_quirk_to_packet(
    mesh_packet: &mut meshtastic_connect::meshtastic::MeshPacket,
    quirks: &Vec<TransportQuirk>,
) {
    for quirk in quirks {
        match quirk {
            TransportQuirk::IncrementHopLimit => mesh_packet.hop_limit += 1,
            TransportQuirk::SetViaMQTT => mesh_packet.via_mqtt = true,
            TransportQuirk::UnsetViaMQTT => mesh_packet.via_mqtt = false,
        }
    }
}
