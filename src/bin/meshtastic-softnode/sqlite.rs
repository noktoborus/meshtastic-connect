use meshtastic_connect::{keyring::node_id::NodeId, meshtastic};
use rusqlite::{Connection, params};

pub(crate) struct SQLite {
    conn: Connection,
}

impl SQLite {
    pub(crate) fn new(db_path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mesh_packets (
                log_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
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
                data BLOB
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    // `port_num.is_some()` indecates that data is not encoded
    pub(crate) fn insert_packet(
        &self,
        packet: &meshtastic::MeshPacket,
        channel_name: Option<String>,
        port_num: Option<meshtastic::PortNum>,
        data: Option<&Vec<u8>>,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO mesh_packets (
                'from', 'to', channel, id, rx_time, rx_snr, hop_limit, want_ack,
                priority, rx_rssi, via_mqtt, hop_start, public_key, pki_encrypted,
                next_hop, relay_node, channel_name, port_num, data
            ) VALUES (?1, ?2, ?3, ?4, DATETIME(?5, 'unixepoch'), ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
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
            ],
        )?;
        Ok(())
    }
}
