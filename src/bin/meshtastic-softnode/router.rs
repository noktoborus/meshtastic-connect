use std::sync::Arc;

use tokio::{sync::Mutex, task::JoinSet};

type Identifier = usize;
use crate::connection::{self, ConnectionAPI};

impl Router {
    pub fn add_connection(&mut self, connection: connection::Connection) {
        self.connections.push(Arc::new(Mutex::new(connection)));
    }

    // Send a mesh packet to all connections except the one specified by `from`
    async fn send_mesh_except(
        &mut self,
        mesh_packet: &meshtastic_connect::meshtastic::MeshPacket,
        from: Option<Identifier>,
    ) {
        for (index, conn) in self.connections.iter_mut().enumerate() {
            if let Some(from) = from {
                if index == from {
                    continue;
                }
            }
            let conn = conn.clone();
            let mesh_packet = mesh_packet.clone();
            tokio::spawn(async move { conn.lock().await.send_mesh(mesh_packet).await });
        }
    }
}

#[derive(Default)]
pub struct Router {
    connections: Vec<Arc<Mutex<connection::Connection>>>,
    recv_set: JoinSet<Result<(Identifier, connection::RecvData), std::io::Error>>,
}

impl ConnectionAPI for Router {
    /// Connect all connections
    async fn connect(&mut self) -> Result<(), std::io::Error> {
        let mut set = JoinSet::new();

        for conn in &mut self.connections {
            let conn = conn.clone();
            set.spawn(async move { conn.lock().await.connect().await });
        }

        while let Some(res) = set.join_next().await {
            match res.map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, format!("thread panicked: {}", e))
            })? {
                Ok(_) => {}
                Err(e) => return Err(e),
            }
        }

        for (identifier, conn) in self.connections.iter_mut().enumerate() {
            let conn = conn.clone();
            self.recv_set
                .spawn(async move { conn.lock().await.recv_mesh().await.map(|r| (identifier, r)) });
        }

        Ok(())
    }

    /// Disconnect all connections
    async fn disconnect(&mut self) {
        for conn in &mut self.connections {
            let conn = conn.clone();
            tokio::spawn(async move { conn.lock().await.disconnect().await });
        }
    }

    // Send to all connections
    async fn send_mesh(
        &mut self,
        mesh_packet: meshtastic_connect::meshtastic::MeshPacket,
    ) -> Result<(), std::io::Error> {
        self.send_mesh_except(&mesh_packet, None).await;
        Ok(())
    }

    // Try to receive from all connections and send to all, except received
    async fn recv_mesh(&mut self) -> Result<connection::RecvData, std::io::Error> {
        while let Some(res) = self.recv_set.join_next().await {
            let (identifier, data) = res.map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, format!("thread panicked: {}", e))
            })??;

            if let connection::RecvData::MeshPacket(ref mesh_packet) = data {
                self.send_mesh_except(mesh_packet, Some(identifier)).await;
            }

            return Ok(data);
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("No connections available"),
        ))
    }
}
