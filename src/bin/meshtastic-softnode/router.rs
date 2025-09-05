use std::sync::Arc;

use tokio::{sync::Mutex, task::JoinSet};

pub type ConnectionName = String;
type ArcConnectionCapsule = Arc<ConnectionCapsule>;
use crate::{
    config::{TransportQuirk, TransportQuirks},
    connection::{self, ConnectionAPI},
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
        channel: Option<String>,
        mesh_packet: &meshtastic_connect::meshtastic::MeshPacket,
        from: Option<ArcConnectionCapsule>,
    ) {
        for capsule in self.connections.iter_mut() {
            if let Some(ref from) = from {
                if capsule.id == from.id {
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
    id: usize,
    name: ConnectionName,
    quirks: TransportQuirks,
    connection: Mutex<connection::Connection>,
}

#[derive(Default)]
pub struct Router {
    connections: Vec<ArcConnectionCapsule>,
    recv_set: JoinSet<Result<(ArcConnectionCapsule, connection::RecvData), std::io::Error>>,
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
            let capsule = capsule.clone();
            self.recv_set.spawn(async move {
                capsule
                    .connection
                    .lock()
                    .await
                    .recv_mesh()
                    .await
                    .map(|r| (capsule.clone(), r))
            });
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
        channel: Option<String>,
        mesh_packet: meshtastic_connect::meshtastic::MeshPacket,
    ) -> Result<(), std::io::Error> {
        self.send_mesh_except(channel, &mesh_packet, None).await;
        Ok(())
    }

    // Try to receive from all connections and send to all, except received
    pub async fn recv_mesh(
        &mut self,
    ) -> Result<(ConnectionName, connection::RecvData), std::io::Error> {
        while let Some(res) = self.recv_set.join_next().await {
            let (capsule, mut data) = res.map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, format!("thread panicked: {}", e))
            })??;

            let connection_name = capsule.name.clone();
            if let connection::RecvData::MeshPacket(ref mut mesh_packet) = data {
                println!("> {:?} received: {:?}", connection_name, mesh_packet);
                apply_quirk_to_packet(mesh_packet, &capsule.quirks.input);
                let channel = if mesh_packet.channel == 0 {
                    None
                } else {
                    Some("".to_string())
                };
                self.send_mesh_except(channel, mesh_packet, Some(capsule.clone()))
                    .await;
            }

            self.recv_set.spawn(async move {
                capsule
                    .connection
                    .lock()
                    .await
                    .recv_mesh()
                    .await
                    .map(|r| (capsule.clone(), r))
            });
            return Ok((connection_name, data));
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("No connections available"),
        ))
    }
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
