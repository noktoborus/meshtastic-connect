mod config;

use clap::Parser;
use config::MQTTConfig;
use meshtastic_connect::{
    keyring::{
        Keyring,
        cryptor::{Decrypt, Encrypt},
        node_id::NodeId,
    },
    meshtastic::{self, ServiceEnvelope, mesh_packet},
    transport::multicast::Multicast,
};

use crate::config::Args;
use crate::config::load_config;
use prost::Message;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Publish, QoS};
use std::{process, time::Duration};

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
    let mut filter_by_nodeid: Vec<NodeId> = Default::default();

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
        if peer.highlight {
            filter_by_nodeid.push(peer.node_id);
        }
    }

    println!();
    let soft_node = config.connection.soft_node;
    let bind_address = soft_node.bind_address;
    println!("Listen multicast on {}", bind_address);
    let mut connection = Multicast::new(bind_address);

    connection.connect().await.unwrap();

    for channel in &soft_node.channels {
        if channel.node_info.is_some() {
            println!("Send initial nodeinfo to {}", channel.name);
            let dest_node: NodeId = 0xffffffff.into();
            let packet_id = 123;
            let node_info = meshtastic::User {
                id: soft_node.node_id.into(),
                long_name: soft_node.name.clone(),
                short_name: soft_node.short_name.clone(),
                hw_model: meshtastic::HardwareModel::AndroidSim.into(),
                is_licensed: false,
                role: meshtastic::config::device_config::Role::Client.into(),
                public_key: vec![],
                ..Default::default()
            };
            let data = meshtastic::Data {
                portnum: meshtastic::PortNum::NodeinfoApp.into(),
                payload: node_info.encode_to_vec(),
                ..Default::default()
            };
            let (cryptor, channel_hash) = keyring
                .cryptor_for_channel_name(soft_node.node_id, &channel.name)
                .unwrap();
            let data = cryptor
                .encrypt(packet_id, data.encode_to_vec())
                .await
                .unwrap();

            let mesh_packet = meshtastic::MeshPacket {
                from: soft_node.node_id.into(),
                to: dest_node.into(),
                channel: channel_hash,
                id: packet_id,
                rx_time: 0,
                rx_snr: 0.0,
                hop_limit: channel.hop_start.into(),
                want_ack: true,
                priority: meshtastic::mesh_packet::Priority::Default.into(),
                rx_rssi: 0,
                via_mqtt: false,
                hop_start: channel.hop_start.into(),
                public_key: vec![],
                pki_encrypted: false,
                next_hop: 0,
                relay_node: 0,
                tx_after: 0,
                payload_variant: Some(mesh_packet::PayloadVariant::Encrypted(data)),
                ..Default::default()
            };

            println!("send mesh: {:?}", mesh_packet);
            connection.send(mesh_packet).await.unwrap();
        }
    }

    loop {
        let (mesh_packet, _) = connection.recv().await.unwrap();

        println!("packet received");
        // print_mesh_packet(mesh_packet, &keyring, &filter_by_nodeid).await;

        println!();
    }
    // let (client_b, mut eventloop_b) = build_connection(config.connection.mqtt_b).await;

    // loop {
    //     select! {
    //         notification = eventloop_a.poll() => {
    //             handle_notifications(notification.unwrap(), &keyring, &client_b).await
    //         }
    //         notification = eventloop_b.poll() => {
    //             handle_notifications(notification.unwrap(), &keyring, &client_a).await
    //         }
    //     }
    // }
}

async fn mesh_packet_get_portnum(
    mesh_packet: meshtastic::MeshPacket,
    keyring: &Keyring,
) -> Option<meshtastic::PortNum> {
    if let Some(payload_variant) = mesh_packet.payload_variant {
        match payload_variant {
            meshtastic::mesh_packet::PayloadVariant::Decoded(data) => Some(data.portnum()),
            meshtastic::mesh_packet::PayloadVariant::Encrypted(items) => {
                let from = mesh_packet.from.into();
                let to = mesh_packet.to.into();
                if let Some(decryptor) = keyring.cryptor_for(from, to, mesh_packet.channel) {
                    match decryptor.decrypt(mesh_packet.id, items).await {
                        Ok(buffer) => {
                            if let Ok(data) = meshtastic::Data::decode(buffer.as_slice()) {
                                Some(data.portnum())
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    }
                } else {
                    None
                }
            }
        }
    } else {
        None
    }
}

async fn process_service_envelope(
    publish: Publish,
    keyring: &Keyring,
    opposite_client: &(AsyncClient, MQTTConfig),
) {
    if let Ok(service) = meshtastic::ServiceEnvelope::decode(publish.payload.clone()) {
        if let Some(mesh_packet) = service.packet {
            let client = &opposite_client.0;
            let from = NodeId::from(mesh_packet.from);
            let to = NodeId::from(mesh_packet.to);
            let mqtt = &opposite_client.1;
            let pass = (mqtt.filter.to.is_empty() || mqtt.filter.to.contains(&to))
                && (mqtt.filter.from.is_empty() || mqtt.filter.from.contains(&from));

            let portnum = mesh_packet_get_portnum(mesh_packet.clone(), keyring).await;

            if pass {
                println!(
                    "[{}:{}] xfer ({} gw {}) {} -> {} {:?}",
                    mqtt.server_addr,
                    mqtt.server_port,
                    publish.topic,
                    service.gateway_id,
                    from,
                    to,
                    portnum.ok_or("<undecrypted>")
                );

                // if mesh_packet.hop_limit == 0 {
                //     mesh_packet.hop_limit = 1
                // }

                let new_service = ServiceEnvelope {
                    packet: Some(mesh_packet),
                    channel_id: service.channel_id,
                    gateway_id: service.gateway_id,
                };

                client
                    .publish(
                        publish.topic,
                        publish.qos,
                        publish.retain,
                        new_service.encode_to_vec(),
                    )
                    .await
                    .unwrap();
            } else {
                println!(
                    "[{}:{}] ignore ({} gw {}) {} -> {} hops: {}/{} decrypt: {:?}",
                    mqtt.server_addr,
                    mqtt.server_port,
                    publish.topic,
                    service.gateway_id,
                    from,
                    to,
                    mesh_packet.hop_limit,
                    mesh_packet.hop_start,
                    portnum.ok_or("<undecrypted>")
                );
            }
        }
    }
}

async fn handle_notifications(
    notification: Event,
    keyring: &Keyring,
    opposite_client: &(AsyncClient, MQTTConfig),
) {
    if let rumqttc::Event::Incoming(packet) = notification {
        match packet {
            rumqttc::Packet::Publish(publish) => {
                process_service_envelope(publish, &keyring, opposite_client).await;
            }
            rumqttc::Packet::PingReq => {}
            rumqttc::Packet::PingResp => {}
            _ => {}
        }
    }
}

async fn build_connection(mqtt: MQTTConfig) -> ((AsyncClient, MQTTConfig), EventLoop) {
    println!(
        "Connect to MQTT {} port {}: {:?}",
        mqtt.server_addr, mqtt.server_port, mqtt.subscribe
    );

    let mut mqttoptions = MqttOptions::new(
        "rumqtt-async",
        mqtt.server_addr.clone(),
        mqtt.server_port.clone(),
    );
    mqttoptions.set_keep_alive(Duration::from_secs(5));
    mqttoptions.set_credentials(mqtt.username.clone(), mqtt.password.clone());

    let (client, connection) = AsyncClient::new(mqttoptions, 10);
    for topic in &mqtt.subscribe {
        client.subscribe(topic, QoS::AtMostOnce).await.unwrap();
    }

    ((client, mqtt), connection)
}
