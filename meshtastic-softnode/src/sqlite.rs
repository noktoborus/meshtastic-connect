use crate::router::ConnectionName;
use chrono::{DateTime, Utc};
use meshtastic_connect::{keyring::node_id::NodeId, meshtastic, transport::mqtt::ConnectionHint};
use prost::Message;
use softnode_client::app::{
    byte_node_id::ByteNodeId,
    data::{DataVariant, DecryptTarget, StoreMeshRxInfo, StoredMeshHeader, StoredMeshPacket},
};
use tokio_rusqlite::{Connection, params};

#[derive(Clone)]
pub(crate) struct SQLite {
    conn: Connection,
}

impl SQLite {
    pub(crate) async fn new(db_path: &str) -> tokio_rusqlite::Result<Self> {
        let conn = Connection::open(db_path).await?;
        conn.call(|conn| {
            Ok(conn.execute(
                "CREATE TABLE IF NOT EXISTS mesh_packets (
                log_time TEXT NOT NULL DEFAULT (STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
                id INTEGER NOT NULL,
                'from' TEXT NOT NULL,
                'to' TEXT NOT NULL,
                channel INTEGER NOT NULL,
                rx_time TEXT NOT NULL,
                rx_snr REAL NOT NULL,
                hop_limit INTEGER NOT NULL,
                want_ack INTEGER NOT NULL,
                priority INTEGER NOT NULL,
                rx_rssi INTEGER NOT NULL,
                via_mqtt INTEGER NOT NULL,
                hop_start INTEGER NOT NULL,
                public_key BLOB,
                pki_encrypted INTEGER NOT NULL,
                next_hop INTEGER NOT NULL,
                relay_node INTEGER NOT NULL,
                channel_name TEXT,
                port_num TEXT,
                data BLOB,
                connection_name TEXT,
                connection_hint TEXT,
                gateway TEXT,
                sequence_number INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT
            )",
                [],
            ))
        })
        .await??;

        Ok(Self { conn })
    }

    pub(crate) async fn select_packets(
        &self,
        from: Option<u64>,
        limit: usize,
    ) -> tokio_rusqlite::Result<Vec<StoredMeshPacket>> {
        self.conn
            .call(move |conn| {
                let query = if let Some(from) = from {
                    format!("SELECT * FROM mesh_packets WHERE sequence_number > {} ORDER BY sequence_number ASC LIMIT {}", from, limit)
                } else {
                    format!(
                        "SELECT * FROM mesh_packets WHERE log_time > datetime('now', '-1 day') ORDER BY sequence_number ASC LIMIT {}",
                        limit
                    )
                };

                let mut stmt = conn.prepare(query.as_str())?;
                let rows = stmt
                    .query_map([], |row| {
                        let from: String = row.get(2)?;
                        let to: String = row.get(3)?;
                        let next_hop: u32 = row.get(15)?;
                        let relay_node: u32 = row.get(16)?;
                        let data: Option<Vec<u8>> = row.get(19)?;
                        let data = if let Some(data) = data {
                            let portnum: Option<String> = row.get(18)?;

                            if portnum.is_some() {
                                let data = meshtastic::Data::decode(data.as_slice())
                                    .map_err(|e| rusqlite::Error::ModuleError(e.to_string()))?;
                                Some(DataVariant::Decrypted(DecryptTarget::Direct(row.get(4)?), data))
                            } else {
                                Some(DataVariant::Encrypted(data))
                            }
                        } else {
                            None
                        };

                        let gateway_or_not: Option<String> = row.get(22)?;
                        let gateway = if let Some(gateway) = gateway_or_not {
                            Some(NodeId::try_from(gateway).unwrap())
                        } else {
                            None
                        };
                        let rx_time: DateTime<Utc> = row.get(5)?;
                        let rx_snr = row.get(6)?;
                        let rx_rssi = row.get(10)?;
                        let rx = if rx_time.timestamp() != 0 || (rx_snr != 0.0 && rx_rssi != 0) {
                            Some(StoreMeshRxInfo {
                                rx_time,
                                rx_snr,
                                rx_rssi,
                            })
                        } else {
                            None
                        };
                        let priority: i32 = row.get(9)?;
                        let priority = match meshtastic::mesh_packet::Priority::try_from(priority)
                        {
                            Ok(priority) => priority.as_str_name().to_string(),
                            Err(_) => {
                                priority.to_string()
                            },
                        };

                        let header = StoredMeshHeader {
                            from: NodeId::try_from(from).unwrap(),
                            to: NodeId::try_from(to).unwrap(),
                            channel: row.get(4)?,
                            id: row.get(1)?,
                            rx,
                            hop_limit: row.get(7)?,
                            // want_ack: row.get(8)?,
                            priority,
                            via_mqtt: row.get(11)?,
                            hop_start: row.get(12)?,
                            // public_key: row.get(13)?,
                            pki_encrypted: row.get(14)?,
                            next_hop: ByteNodeId::from(next_hop),
                            relay_node: ByteNodeId::from(relay_node),
                        };

                        Ok(StoredMeshPacket {
                            sequence_number: row.get(23)?,
                            gateway,
                            store_timestamp: row.get(0)?,
                            connection_name: row.get(20)?,
                            connection_hint: row.get(21)?,
                            header,
                            data,
                        })
                    })
                    .map_err(|e| tokio_rusqlite::Error::Rusqlite(e))?;

                let mut list = Vec::new();

                for row in rows {
                    match row {
                        Ok(row) => list.push(row),
                        Err(e) => {
                            println!("row process error: {}", e);
                            continue;
                        }
                    }
                }

                Ok(list)
            })
            .await
    }

    // `port_num.is_some()` indecates that data is not encoded
    pub(crate) async fn insert_packet(
        &self,
        gateway: Option<NodeId>,
        connection_name: &ConnectionName,
        connection_hint: Option<ConnectionHint>,
        packet: &meshtastic::MeshPacket,
        channel_name: Option<String>,
        port_num: Option<meshtastic::PortNum>,
        data: Option<&Vec<u8>>,
    ) -> tokio_rusqlite::Result<()> {
        let connection_name = connection_name.clone();
        let connection_hint = connection_hint.clone();
        let packet = packet.clone();
        let data = if let Some(data) = data {
            Some(data.clone())
        } else {
            None
        };

        self.conn.call(move |conn|  {
            Ok(conn.execute(
            "INSERT INTO mesh_packets (
                'from', 'to', channel, id, rx_time, rx_snr, hop_limit, want_ack,
                priority, rx_rssi, via_mqtt, hop_start, public_key, pki_encrypted,
                next_hop, relay_node, channel_name, port_num, data, connection_name, connection_hint, gateway
            ) VALUES (?1, ?2, ?3, ?4, DATETIME(?5, 'unixepoch'), ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
            params![
                NodeId::from(packet.from).to_string(),
                NodeId::from(packet.to).to_string(),
                packet.channel,
                packet.id,
                packet.rx_time,
                packet.rx_snr,
                packet.hop_limit,
                packet.want_ack as i32,
                packet.priority,
                packet.rx_rssi,
                packet.via_mqtt as i32,
                packet.hop_start,
                packet.public_key,
                packet.pki_encrypted as i32,
                packet.next_hop,
                packet.relay_node,
                channel_name,
                port_num.map(|v| v.as_str_name()),
                data,
                connection_name,
                connection_hint,
                gateway.map(|v| v.to_string()),
            ],
        ))
        }).await??;
        Ok(())
    }
}
