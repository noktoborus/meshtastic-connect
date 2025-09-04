use std::sync::Arc;

use tokio::{sync::Mutex, task::JoinSet};

pub type ConnectionName = String;
type Identifier = usize;
use crate::connection::{self, ConnectionAPI};

impl Router {
    pub fn add_connection(&mut self, connection_name: String, connection: connection::Connection) {
        self.connections.push(Arc::new(ConnectionCapsule {
            name: connection_name,
            connection: Mutex::new(connection),
        }));
    }

    // Send a mesh packet to all connections except the one specified by `from`
    async fn send_mesh_except(
        &mut self,
        channel: Option<String>,
        mesh_packet: &meshtastic_connect::meshtastic::MeshPacket,
        from: Option<Identifier>,
    ) {
        for (index, capsule) in self.connections.iter_mut().enumerate() {
            if let Some(from) = from {
                if index == from {
                    continue;
                }
            }
            let capsule = capsule.clone();
            let mesh_packet = mesh_packet.clone();
            let channel = channel.clone();
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

pub struct ConnectionCapsule {
    name: ConnectionName,
    connection: Mutex<connection::Connection>,
}

#[derive(Default)]
pub struct Router {
    connections: Vec<Arc<ConnectionCapsule>>,
    recv_set: JoinSet<Result<(Identifier, connection::RecvData), std::io::Error>>,
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

        for (identifier, capsule) in self.connections.iter_mut().enumerate() {
            let capsule = capsule.clone();
            self.recv_set.spawn(async move {
                capsule
                    .connection
                    .lock()
                    .await
                    .recv_mesh()
                    .await
                    .map(|r| (identifier, r))
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
            let (identifier, data) = res.map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, format!("thread panicked: {}", e))
            })??;

            if let connection::RecvData::MeshPacket(ref mesh_packet) = data {
                let channel = if mesh_packet.channel == 0 {
                    None
                } else {
                    Some("".to_string())
                };
                self.send_mesh_except(channel, mesh_packet, Some(identifier))
                    .await;
            }

            let capsule = &self.connections[identifier];
            return Ok((capsule.name.clone(), data));
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("No connections available"),
        ))
    }
}
