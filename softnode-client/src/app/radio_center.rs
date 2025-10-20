use std::collections::HashMap;

use geo::Point;
use geo::algorithm::centroid::Centroid;
use meshtastic_connect::keyring::node_id::NodeId;

use crate::app::data::{GatewayInfo, NodeInfo};

fn rssi_to_distance(rssi: i32) -> f64 {
    let clamped = rssi.clamp(-130, 20);
    let normalized = (20 - clamped) as f64 / 150.0;
    normalized * 1.0
}

fn offset_point(
    from: walkers::Position,
    to: walkers::Position,
    distance_km: f64,
) -> walkers::Position {
    let dx = to.x() - from.x();
    let dy = to.y() - from.y();
    let len = (dx * dx + dy * dy).sqrt();

    if len == 0.0 {
        return to;
    }

    let unit_x = dx / len;
    let unit_y = dy / len;

    Point::new(to.x() + unit_x * distance_km, to.y() + unit_y * distance_km)
}

// Want audit: it is a LLM's code
pub fn compute_weighted_center(
    positions: Vec<(i32, walkers::Position)>,
) -> Option<walkers::Position> {
    if let Some(initial_center) =
        geo::MultiPoint::from(positions.iter().map(|(_, p)| *p).collect::<Vec<_>>()).centroid()
    {
        let shifted_points: Vec<walkers::Position> = positions
            .iter()
            .map(|(rssi, point)| {
                let dist = rssi_to_distance(*rssi);
                offset_point(initial_center, *point, dist)
            })
            .collect();

        geo::MultiPoint::from(shifted_points).centroid()
    } else {
        None
    }
}

pub fn assume_position(
    node_info: &NodeInfo,
    nodes: &HashMap<NodeId, NodeInfo>,
) -> Option<walkers::Position> {
    let to_pos_info = |node_id,
                       gateway_info: Option<&GatewayInfo>|
     -> Option<(i32, walkers::Position)> {
        if let Some(gateway_info) = gateway_info {
            nodes
                .get(node_id)
                .map(|v| {
                    gateway_info
                        .rx_info
                        .as_ref()
                        .map(|rx_info| {
                            if let Some(position) = v.position.last() {
                                Some((
                                    rx_info.rx_rssi,
                                    walkers::Position::new(position.longitude, position.latitude),
                                ))
                            } else {
                                None
                            }
                        })
                        .flatten()
                })
                .flatten()
        } else {
            None
        }
    };

    let mut positions = node_info
        .gatewayed_by
        .iter()
        .map(|(id, info)| to_pos_info(id, Some(info)))
        .filter(|v| v.is_some())
        .flatten()
        .collect::<Vec<_>>();
    positions.append(
        &mut node_info
            .gateway_for
            .iter()
            .map(|(id, info)| to_pos_info(id, info.last()))
            .filter(|v| v.is_some())
            .flatten()
            .collect::<Vec<_>>(),
    );

    compute_weighted_center(positions)
}
