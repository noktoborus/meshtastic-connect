mod config;
mod connection;
mod publish;
mod router;
mod schedule;
mod sqlite;

use clap::Parser;
use meshtastic_connect::{
    keyring::{
        Keyring,
        cryptor::{Decrypt, Encrypt},
        node_id::NodeId,
    },
    meshtastic::{self, mesh_packet},
};
use prost::Message;
use publish::Publishable;
use rand::Rng;
use std::{
    process::{self, exit},
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    time::{Instant, sleep_until},
};

use crate::config::{Args, SoftNodeConfig, load_config};

async fn handle_timer_event(
    sqlite: &sqlite::SQLite,
    schedule: &mut schedule::Schedule,
    soft_node: &SoftNodeConfig,
    keyring: &Keyring,
    router: &mut router::Router,
) {
    while let Some((_, (channel_idx, publish_idx))) = schedule.pop_if_completed() {
        let channel = &soft_node.channels[channel_idx];
        let publish_descriptor = &channel.publish[publish_idx];

        println!(
            "Publishing {:?} to channel {}",
            publish_descriptor, channel.name
        );
        let (port_num, data_payload) = publish_descriptor.pack_to_data(&soft_node);
        let packet_id: u32 = rand::rng().random();
        let dest_node: u32 = 0xffffffff;
        let data = meshtastic::Data {
            portnum: port_num.into(),
            payload: data_payload,
            ..Default::default()
        };

        let (channel_hash, payload_variant) = if channel.disable_encryption {
            (
                channel_idx as u32,
                mesh_packet::PayloadVariant::Decoded(data.clone()),
            )
        } else {
            let (cryptor, channel_hash) = keyring
                .cryptor_for_channel_name(soft_node.node_id, &channel.name)
                .unwrap();

            let encrypted_data = cryptor
                .encrypt(packet_id, data.encode_to_vec())
                .await
                .unwrap();
            (
                channel_hash,
                mesh_packet::PayloadVariant::Encrypted(encrypted_data),
            )
        };

        let mesh_packet = meshtastic::MeshPacket {
            from: soft_node.node_id.into(),
            to: dest_node.into(),
            channel: channel_hash,
            id: packet_id,
            hop_limit: channel.hop_start.into(),
            priority: meshtastic::mesh_packet::Priority::Default.into(),
            hop_start: channel.hop_start.into(),
            payload_variant: Some(payload_variant),
            ..Default::default()
        };

        println!("send mesh: {:?}", mesh_packet);
        sqlite
            .insert_packet(
                &soft_node.node_id.to_string(),
                &mesh_packet,
                Some(channel.name.clone()),
                Some(port_num),
                Some(&data.encode_to_vec()),
            )
            .unwrap();
        router
            .send_mesh(Some(channel.name.clone()), mesh_packet)
            .await;

        let interval = publish_descriptor.interval();
        if !interval.is_zero() {
            schedule.add(Instant::now() + interval, (channel_idx, publish_idx));
        }
    }
}

async fn handle_network_event(
    sqlite: &sqlite::SQLite,
    keyring: &Keyring,
    router: &mut router::Router,
    recv_capsule: router::ReceiveCapsule,
) {
    match &recv_capsule.incoming.data {
        connection::DataVariant::MeshPacket(mesh_packet) => {
            if let Some(ref payload_variant) = mesh_packet.payload_variant {
                match payload_variant {
                    mesh_packet::PayloadVariant::Decoded(data) => {
                        sqlite
                            .insert_packet(
                                &recv_capsule.source_connection_name,
                                &mesh_packet,
                                None,
                                Some(data.portnum()),
                                Some(&data.encode_to_vec()),
                            )
                            .unwrap();
                        // router: get channel name by mesh_packet.channel (number of channel)
                    }
                    mesh_packet::PayloadVariant::Encrypted(encrypted_data) => {
                        if let Some((cryptor, data)) = match keyring.cryptor_for(
                            NodeId::from(mesh_packet.from),
                            NodeId::from(mesh_packet.to),
                            mesh_packet.channel,
                        ) {
                            Some(cryptor) => {
                                match cryptor
                                    .decrypt(mesh_packet.id, encrypted_data.clone())
                                    .await
                                {
                                    Ok(decrypted_data) => {
                                        match meshtastic::Data::decode(decrypted_data.as_slice()) {
                                            Ok(data) => Some((cryptor, data)),
                                            Err(err) => {
                                                println!("Failed to construct data: {}", err);
                                                None
                                            }
                                        }
                                    }
                                    Err(err) => {
                                        println!("Failed to decrypt encrypted data: {}", err);
                                        None
                                    }
                                }
                            }
                            None => {
                                println!("No cryptor found for packet: {:?}", mesh_packet);
                                None
                            }
                        } {
                            sqlite
                                .insert_packet(
                                    &recv_capsule.source_connection_name,
                                    &mesh_packet,
                                    Some(cryptor.to_string()),
                                    Some(data.portnum()),
                                    Some(&data.encode_to_vec()),
                                )
                                .unwrap();
                            // router
                            //     .route_next(Some(cryptor.to_string()), recv_capsule)
                            //     .await;
                        } else {
                            sqlite
                                .insert_packet(
                                    &recv_capsule.source_connection_name,
                                    &mesh_packet,
                                    None,
                                    None,
                                    Some(encrypted_data),
                                )
                                .unwrap();
                            // let channel = if mesh_packet.pki_encrypted {
                            //     Some("PKI".into())
                            // } else {
                            //     None
                            // };
                            // router.route_next(channel, recv_capsule).await;
                        };
                    }
                }
            } else {
                println!("No data received: {:?}", mesh_packet);
                sqlite
                    .insert_packet(
                        &recv_capsule.source_connection_name,
                        &mesh_packet,
                        None,
                        None,
                        None,
                    )
                    .unwrap();
            }
        }
        connection::DataVariant::Unstructured(items) => {
            tokio::io::stderr().write_all(&items).await.unwrap()
        }
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let config = load_config(&args).unwrap_or_else(|| {
        println!("Config file not loaded: try type `--help` to get help");
        process::exit(1)
    });

    println!("=== loaded config ===");
    println!("{}", serde_yaml_ng::to_string(&config).unwrap());
    println!("=== ===");

    let mut keyring = Keyring::new();

    for channel in config.keys.channels {
        keyring
            .add_channel(channel.name.as_str(), channel.key)
            .unwrap();
    }

    for peer in config.keys.peers {
        if let Some(skey) = peer.private_key {
            keyring.add_peer(peer.node_id, skey).unwrap();
        } else if let Some(pkey) = peer.public_key {
            keyring.add_remote_peer(peer.node_id, pkey).unwrap();
        }
    }

    println!();
    let soft_node = config.soft_node;
    let mut schedule = schedule::Schedule::new(&soft_node.channels);
    let sqlite_name = format!("journal-{:x}.sqlite", soft_node.node_id);
    let sqlite = sqlite::SQLite::new(sqlite_name.as_str()).unwrap();
    let mut router = router::Router::default();

    for transport in &soft_node.transport {
        router.add_connection(
            transport.name.clone(),
            transport.quirks.clone(),
            soft_node.default_channel.clone(),
            connection::build(transport.clone(), &soft_node).await,
        );
    }

    loop {
        let next_wakeup = schedule.next_wakeup().unwrap_or_else(|| {
            Instant::now() + Duration::from_secs(60 * 60 * 24) // 1 day
        });

        tokio::select! {
            _ = sleep_until(next_wakeup) => {
                handle_timer_event(&sqlite, &mut schedule, &soft_node, &keyring, &mut router).await;
            },
            result = router.recv_mesh() => {
                match result {
                    Ok(recv_capsule) => { handle_network_event(&sqlite, &keyring, &mut router, recv_capsule).await; }
                    Err(err) => {
                        println!("handle error: {}", err);
                        exit(1);
                    }
                }
            }
        }
    }
}
