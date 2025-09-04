use crate::keyring::cryptor::Decrypt;
use crate::{
    keyring::Keyring,
    meshtastic::{self, Data, MeshPacket, from_radio},
};
use bytes::Bytes;
use chrono::{TimeZone, Utc};
use prost::Message;

async fn print_decoded(data: Data) -> Result<(), String> {
    println!(
        "- {:?} paylen={} source={:#x} dest={:#x} want_response={}, reply_id={}, request_id={} emoji={:#x}",
        data.portnum(),
        data.payload.len(),
        data.source,
        data.dest,
        data.want_response,
        data.reply_id,
        data.request_id,
        data.emoji
    );
    match data.portnum() {
        meshtastic::PortNum::TextMessageApp => {
            println!("{{ {} }}", String::from_utf8_lossy(data.payload.as_slice()));
        }
        meshtastic::PortNum::PositionApp => {
            let position =
                meshtastic::Position::decode(data.payload.as_slice()).map_err(|e| e.to_string())?;
            println!("{{ {} }}", position);
        }
        meshtastic::PortNum::NodeinfoApp => {
            let node_info =
                meshtastic::User::decode(data.payload.as_slice()).map_err(|e| e.to_string())?;
            println!("{{ {} }}", node_info);
        }
        meshtastic::PortNum::TelemetryApp => {
            let telemetry = meshtastic::Telemetry::decode(data.payload.as_slice())
                .map_err(|e| e.to_string())?;
            println!("- TelemetryApp {{ {} }}", telemetry);
        }
        meshtastic::PortNum::RangeTestApp => {
            println!("{{ {} }}", String::from_utf8_lossy(data.payload.as_slice()));
        }
        meshtastic::PortNum::StoreForwardApp => {
            let sf = meshtastic::StoreAndForward::decode(data.payload.as_slice())
                .map_err(|e| e.to_string())?;

            println!("{{ {:?} }}", sf);
        }
        meshtastic::PortNum::NeighborinfoApp => {
            let neighbor_info = meshtastic::NeighborInfo::decode(data.payload.as_slice())
                .map_err(|e| e.to_string())?;

            println!("{{ {} }}", neighbor_info);
        }
        meshtastic::PortNum::WaypointApp => {
            let waypoint =
                meshtastic::Waypoint::decode(data.payload.as_slice()).map_err(|e| e.to_string())?;

            println!("{{ {:?} }}", waypoint);
        }
        meshtastic::PortNum::AdminApp => {
            let admin = meshtastic::AdminMessage::decode(data.payload.as_slice())
                .map_err(|e| e.to_string())?;

            println!("{{ {} }}", admin);
        }
        _ => {
            println!("{{ <todo> }}");
        }
    }
    Ok(())
}

pub async fn print_mesh_packet(mesh_packet: MeshPacket, channel_list: &Keyring) {
    let from_formatted = mesh_packet.from.to_string();
    let to_formatted = mesh_packet.to.to_string();

    println!(
        "- from={} to={} channel=0x{:0>2x} [id:{}]{} hop={{{}/{}}} want_ack={} (PKI ENC={})",
        from_formatted,
        to_formatted,
        mesh_packet.channel,
        mesh_packet.id,
        if mesh_packet.via_mqtt { " MQTT" } else { "" },
        mesh_packet.hop_limit,
        mesh_packet.hop_start,
        mesh_packet.want_ack,
        mesh_packet.pki_encrypted
    );

    if let chrono::LocalResult::Single(time) = Utc.timestamp_opt(mesh_packet.rx_time as i64, 0) {
        println!("- RX Time: {}", time.format("%Y-%m-%d %H:%M:%S UTC"));
    }

    if mesh_packet.rx_snr != 0.0 || mesh_packet.rx_rssi != 0 {
        println!(
            "- SNR: {:.1} dB, RSSI: {} dBm",
            mesh_packet.rx_snr, mesh_packet.rx_rssi
        );
    }

    if let Some(payload_variant) = mesh_packet.payload_variant {
        match payload_variant {
            meshtastic::mesh_packet::PayloadVariant::Decoded(data) => {
                match print_decoded(data).await {
                    Ok(_) => {}
                    Err(e) => {
                        println!("! [construct error] {:?}", e)
                    }
                }
            }
            meshtastic::mesh_packet::PayloadVariant::Encrypted(items) => {
                let from = mesh_packet.from.into();
                let to = mesh_packet.to.into();
                let decryptor = channel_list.cryptor_for(from, to, mesh_packet.channel);

                if decryptor.is_none() {
                    println!(
                        "Not found decoding info for <{} → {} chan {:#x}>",
                        from, to, mesh_packet.channel
                    );
                    return;
                }
                let decryptor = decryptor.unwrap();
                println!("  <decrypting {} bytes for {}>", items.len(), decryptor);

                match decryptor.decrypt(mesh_packet.id, items).await {
                    Ok(buffer) => match meshtastic::Data::decode(buffer.as_slice()) {
                        Ok(data) => match print_decoded(data).await {
                            Ok(_) => {}
                            Err(e) => {
                                println!("! [print error] {:?}", e)
                            }
                        },
                        Err(e) => {
                            println!("! [construct error] Unable to construct `Data`: {:?}", e);
                        }
                    },
                    Err(e) => println!("! [decode error] {:?}", e),
                }
            }
        }
    }
}

pub async fn print_service_envelope(packet: Bytes, channel_list: &Keyring) {
    if let Ok(service) = meshtastic::ServiceEnvelope::decode(packet.clone()) {
        if let Some(mesh_packet) = service.packet {
            println!("- chan={:?} gw={}", service.channel_id, service.gateway_id,);

            print_mesh_packet(mesh_packet, channel_list).await;
        } else {
            println!(
                "- chan={:?} gw={} <no data>",
                service.channel_id, service.gateway_id
            );
        }
        println!("");
    }
}

pub async fn print_from_radio_payload(payload: from_radio::PayloadVariant, channel_list: &Keyring) {
    match payload {
        from_radio::PayloadVariant::Packet(mesh_packet) => {
            print_mesh_packet(mesh_packet, channel_list).await
        }
        from_radio::PayloadVariant::LogRecord(log_record) => {
            println!("- LogRecord {{ {:?} }}", log_record)
        }
        from_radio::PayloadVariant::ConfigCompleteId(config_complete_id) => {
            println!("- ConfigCompleteId {:#x}", config_complete_id)
        }
        from_radio::PayloadVariant::NodeInfo(node_info) => {
            println!("- NodeInfo");
            println!("{{ {} }}", node_info);
        }
        other => {
            println!("- {:?}", other);
        }
    }
}
