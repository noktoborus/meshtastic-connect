use std::fmt;

use base64::{Engine, engine::general_purpose};
use chrono::{TimeZone, Utc};

use crate::meshtastic;

impl fmt::Display for meshtastic::telemetry::Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            meshtastic::telemetry::Variant::HealthMetrics(hm) => hm.fmt(f)?,
            meshtastic::telemetry::Variant::HostMetrics(hm) => hm.fmt(f)?,
            meshtastic::telemetry::Variant::DeviceMetrics(device_metrics) => {
                device_metrics.fmt(f)?
            }
            meshtastic::telemetry::Variant::EnvironmentMetrics(environment_metrics) => {
                environment_metrics.fmt(f)?
            }
            meshtastic::telemetry::Variant::AirQualityMetrics(air_quality_metrics) => {
                air_quality_metrics.fmt(f)?
            }
            meshtastic::telemetry::Variant::PowerMetrics(power_metrics) => power_metrics.fmt(f)?,
            meshtastic::telemetry::Variant::LocalStats(local_stats) => local_stats.fmt(f)?,
        }
        Ok(())
    }
}

impl fmt::Display for meshtastic::LocalStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "📡 Local Mesh Stats:")?;
        writeln!(f, "  ⏱️ Uptime: {} seconds", self.uptime_seconds)?;
        writeln!(
            f,
            "  📶 Channel Utilization: {:.1}%",
            self.channel_utilization
        )?;
        writeln!(f, "  📡 TX Air Utilization: {:.1}%", self.air_util_tx)?;
        writeln!(f, "  📤 Packets Sent: {}", self.num_packets_tx)?;
        writeln!(f, "  📥 Packets Received: {}", self.num_packets_rx)?;
        writeln!(f, "  ❌ Malformed Packets: {}", self.num_packets_rx_bad)?;
        writeln!(f, "  🟢 Online Nodes (2h): {}", self.num_online_nodes)?;
        writeln!(f, "  🌐 Total Nodes: {}", self.num_total_nodes)?;
        writeln!(f, "  🔁 Duplicate RX Packets: {}", self.num_rx_dupe)?;
        writeln!(f, "  🚚 TX Relayed Packets: {}", self.num_tx_relay)?;
        writeln!(f, "  🛑 TX Relay Canceled: {}", self.num_tx_relay_canceled)?;
        writeln!(f, "  🧵 Heap Used: {} bytes", self.heap_total_bytes)?;
        writeln!(f, "  🧵 Heap Free: {} bytes", self.heap_free_bytes)?;
        Ok(())
    }
}

impl fmt::Display for meshtastic::AirQualityMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "🌫️ Качество воздуха:")?;
        if let Some(v) = self.pm10_standard {
            writeln!(f, "  🧪 PM1.0 (стандарт): {} μg/m³", v)?;
        }
        if let Some(v) = self.pm25_standard {
            writeln!(f, "  🧪 PM2.5 (стандарт): {} μg/m³", v)?;
        }
        if let Some(v) = self.pm100_standard {
            writeln!(f, "  🧪 PM10.0 (стандарт): {} μg/m³", v)?;
        }
        if let Some(v) = self.pm10_environmental {
            writeln!(f, "  🌍 PM1.0 (эколог): {} μg/m³", v)?;
        }
        if let Some(v) = self.pm25_environmental {
            writeln!(f, "  🌍 PM2.5 (эколог): {} μg/m³", v)?;
        }
        if let Some(v) = self.pm100_environmental {
            writeln!(f, "  🌍 PM10.0 (эколог): {} μg/m³", v)?;
        }
        if let Some(v) = self.co2 {
            writeln!(f, "  🌬️ CO₂: {} ppm", v)?;
        }
        // Отображение частиц
        if let Some(v) = self.particles_03um {
            writeln!(f, "  ⚛️ Частицы ≥0.3μm: {}", v)?;
        }
        if let Some(v) = self.particles_05um {
            writeln!(f, "  ⚛️ Частицы ≥0.5μm: {}", v)?;
        }
        if let Some(v) = self.particles_10um {
            writeln!(f, "  ⚛️ Частицы ≥1.0μm: {}", v)?;
        }
        if let Some(v) = self.particles_25um {
            writeln!(f, "  ⚛️ Частицы ≥2.5μm: {}", v)?;
        }
        if let Some(v) = self.particles_50um {
            writeln!(f, "  ⚛️ Частицы ≥5.0μm: {}", v)?;
        }
        if let Some(v) = self.particles_100um {
            writeln!(f, "  ⚛️ Частицы ≥10.0μm: {}", v)?;
        }
        Ok(())
    }
}

impl fmt::Display for meshtastic::HostMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "💻 Хост-система:")?;
        writeln!(f, "  ⏱️ Аптайм: {} сек", self.uptime_seconds)?;
        writeln!(f, "  🧠 Свободная память: {} Б", self.freemem_bytes)?;
        writeln!(f, "  💾 Диск / свободен: {} Б", self.diskfree1_bytes)?;
        if let Some(d2) = self.diskfree2_bytes {
            writeln!(f, "  📁 Диск 2 свободен: {} Б", d2)?;
        }
        if let Some(d3) = self.diskfree3_bytes {
            writeln!(f, "  📂 Диск 3 свободен: {} Б", d3)?;
        }
        writeln!(
            f,
            "  📊 Нагрузка: 1мин={}  5мин={}  15мин={}",
            self.load1, self.load5, self.load15
        )?;
        if let Some(user_str) = &self.user_string {
            writeln!(f, "  📝 Пользовательская строка: {}", user_str)?;
        }
        Ok(())
    }
}

impl fmt::Display for meshtastic::PowerMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "⚡️ Энергетические метрики:")?;
        if let Some(v) = self.ch1_voltage {
            writeln!(f, "  🔌 Напряжение Ch1: {:.2} V", v)?;
        }
        if let Some(c) = self.ch1_current {
            writeln!(f, "  ⚡️ Ток Ch1: {:.2} A", c)?;
        }
        if let Some(v) = self.ch2_voltage {
            writeln!(f, "  🔌 Напряжение Ch2: {:.2} V", v)?;
        }
        if let Some(c) = self.ch2_current {
            writeln!(f, "  ⚡️ Ток Ch2: {:.2} A", c)?;
        }
        if let Some(v) = self.ch3_voltage {
            writeln!(f, "  🔌 Напряжение Ch3: {:.2} V", v)?;
        }
        if let Some(c) = self.ch3_current {
            writeln!(f, "  ⚡️ Ток Ch3: {:.2} A", c)?;
        }

        Ok(())
    }
}

impl fmt::Display for meshtastic::HealthMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "💊 Здоровье:")?;
        if let Some(bpm) = self.heart_bpm {
            writeln!(f, "  ❤️ Пульс: {} BPM", bpm)?;
        }
        if let Some(spo2) = self.sp_o2 {
            writeln!(f, "  🩸 SpO₂: {}%", spo2)?;
        }
        if let Some(temp) = self.temperature {
            writeln!(f, "  🌡️ Температура тела: {:.1} °C", temp)?;
        }

        Ok(())
    }
}

impl fmt::Display for meshtastic::DeviceMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "🔧 Device Metrics:")?;
        if let Some(batt) = self.battery_level {
            writeln!(f, "  🔋 Battery Level: {}%", batt)?;
        }
        if let Some(voltage) = self.voltage {
            writeln!(f, "  ⚡️ Voltage: {:.2} V", voltage)?;
        }
        if let Some(util) = self.channel_utilization {
            writeln!(f, "  📶 Channel Utilization: {:.1}%", util)?;
        }
        if let Some(tx) = self.air_util_tx {
            writeln!(f, "  📡 TX Air Utilization: {:.1}%", tx)?;
        }
        if let Some(uptime) = self.uptime_seconds {
            writeln!(f, "  ⏱️ Uptime: {} seconds", uptime)?;
        }
        Ok(())
    }
}

impl fmt::Display for meshtastic::EnvironmentMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "🌦 Environment Metrics:")?;
        if let Some(temp) = self.temperature {
            writeln!(f, "  🌡 Temperature: {:.1}°C", temp)?;
        }
        if let Some(hum) = self.relative_humidity {
            writeln!(f, "  💧 Humidity: {:.1}%", hum)?;
        }
        if let Some(press) = self.barometric_pressure {
            writeln!(f, "  🧭 Pressure: {:.1} hPa", press)?;
        }
        if let Some(gas) = self.gas_resistance {
            writeln!(f, "  🧪 Gas Resistance: {:.2} MΩ", gas)?;
        }
        if let Some(voltage) = self.voltage {
            writeln!(f, "  ⚡️ Voltage: {:.2} V", voltage)?;
        }
        if let Some(current) = self.current {
            writeln!(f, "  🔌 Current: {:.2} A", current)?;
        }
        if let Some(iaq) = self.iaq {
            writeln!(f, "  🌫 IAQ: {}", iaq)?;
        }
        if let Some(dist) = self.distance {
            writeln!(f, "  🌊 Distance: {:.1} mm", dist)?;
        }
        if let Some(lux) = self.lux {
            writeln!(f, "  💡 Ambient Light: {:.1} lx", lux)?;
        }
        if let Some(white) = self.white_lux {
            writeln!(f, "  📃 White Lux: {:.1}", white)?;
        }
        if let Some(ir) = self.ir_lux {
            writeln!(f, "  🔴 IR Lux: {:.1}", ir)?;
        }
        if let Some(uv) = self.uv_lux {
            writeln!(f, "  🟣 UV Lux: {:.1}", uv)?;
        }
        if let Some(wind_dir) = self.wind_direction {
            writeln!(f, "  🧭 Wind Direction: {}°", wind_dir)?;
        }
        if let Some(wind_speed) = self.wind_speed {
            writeln!(f, "  💨 Wind Speed: {:.1} m/s", wind_speed)?;
        }
        if let Some(weight) = self.weight {
            writeln!(f, "  ⚖️ Weight: {:.2} kg", weight)?;
        }
        if let Some(gust) = self.wind_gust {
            writeln!(f, "  🌬 Wind Gust: {:.1} m/s", gust)?;
        }
        if let Some(lull) = self.wind_lull {
            writeln!(f, "  🍃 Wind Lull: {:.1} m/s", lull)?;
        }
        if let Some(rad) = self.radiation {
            writeln!(f, "  ☢️ Radiation: {:.2} µR/h", rad)?;
        }
        if let Some(rain) = self.rainfall_1h {
            writeln!(f, "  🌧 Rainfall (1h): {:.1} mm", rain)?;
        }
        Ok(())
    }
}

impl fmt::Display for meshtastic::Telemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.time == 0 {
            writeln!(f, "🕒 Время: неизвестно")?;
        } else {
            let ts = self.time as i64 as i64;

            match Utc.timestamp_opt(ts, 0) {
                chrono::offset::LocalResult::Single(dt) => {
                    writeln!(f, "🕒 Время: {}", dt.format("%Y-%m-%d %H:%M:%S UTC"))?;
                }
                chrono::offset::LocalResult::Ambiguous(_, _) => todo!(),
                chrono::offset::LocalResult::None => todo!(),
            }
        }

        if let Some(variant) = &self.variant {
            writeln!(f, " {}", variant)?;
        } else {
            writeln!(f, " ⚠️ Нет данных variant")?;
        }

        Ok(())
    }
}

impl fmt::Display for meshtastic::Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "  ")?;

        if self.timestamp > 0 {
            if let chrono::LocalResult::Single(ts) = Utc.timestamp_opt(self.timestamp as i64, 0) {
                write!(
                    f,
                    "🕒 GPS Timestamp: {} ",
                    ts.format("%Y-%m-%d %H:%M:%S UTC")
                )?;
            }
        }

        if self.time > 0 {
            if let chrono::LocalResult::Single(dt) = Utc.timestamp_opt(self.time as i64, 0) {
                write!(f, "⏰ System Time: {}", dt.format("%Y-%m-%d %H:%M:%S UTC"))?;
            }
        }

        if self.timestamp > 0 || self.time > 0 {
            writeln!(f, "")?;
        }

        if let (Some(lat), Some(lon)) = (self.latitude_i, self.longitude_i) {
            write!(f, "  🌐 {:.7} {:.7}", lat as f64 * 1e-7, lon as f64 * 1e-7)?;
        }

        writeln!(f, " 🛰 Satellites in View: {}", self.sats_in_view)?;

        if let Some(alt) = self.altitude {
            writeln!(f, "  🗻 Altitude (MSL): {} m", alt)?; // Don't mark as empty, even if 0
        }
        if let Some(hae) = self.altitude_hae {
            writeln!(f, "  🛰 Altitude (HAE): {} m", hae)?;
        }
        if let Some(geo) = self.altitude_geoidal_separation {
            writeln!(f, "  🌎 Geoidal Separation: {} m", geo)?;
        }

        if self.timestamp_millis_adjust != 0 {
            writeln!(
                f,
                "  🔧 Timestamp Adjustment: {} ms",
                self.timestamp_millis_adjust
            )?;
        }

        if self.location_source != 0 {
            writeln!(
                f,
                "  🎯 Location Source: {}",
                meshtastic::position::LocSource::try_from(self.location_source)
                    .unwrap()
                    .as_str_name()
            )?;
        }

        if self.altitude_source != 0 {
            writeln!(
                f,
                "  🗺 Altitude Source: {}",
                meshtastic::position::AltSource::try_from(self.altitude_source)
                    .unwrap()
                    .as_str_name()
            )?;
        }

        if self.pdop != 0 {
            writeln!(f, "  📡 PDOP: {:.2}", self.pdop as f64 / 100.0)?;
        }

        if self.hdop != 0 {
            writeln!(f, "  📡 HDOP: {:.2}", self.hdop as f64 / 100.0)?;
        }

        if self.vdop != 0 {
            writeln!(f, "  📡 VDOP: {:.2}", self.vdop as f64 / 100.0)?;
        }

        if self.gps_accuracy != 0 {
            writeln!(f, "  🎯 GPS Accuracy: {} mm", self.gps_accuracy)?;
        }

        if let Some(speed) = self.ground_speed {
            if speed != 0 {
                writeln!(f, "  🚀 Ground Speed: {:.2} m/s", speed as f64)?;
            }
        }

        if let Some(track) = self.ground_track {
            if track != 0 {
                writeln!(f, "  🧭 Ground Track: {:.2}°", track as f64 / 100.0)?;
            }
        }

        if self.fix_quality != 0 {
            writeln!(f, "  📶 Fix Quality: {}", self.fix_quality)?;
        }

        if self.fix_type != 0 {
            writeln!(f, "  📶 Fix Type: {}", self.fix_type)?;
        }

        if self.sensor_id != 0 {
            writeln!(f, "  🆔 Sensor ID: {}", self.sensor_id)?;
        }

        if self.next_update != 0 {
            writeln!(f, "  ⏳ Next Update In: {} seconds", self.next_update)?;
        }

        if self.seq_number != 0 {
            writeln!(f, "  🔢 Sequence Number: {}", self.seq_number)?;
        }

        if self.precision_bits != 0 {
            writeln!(f, "  🧬 Precision Bits: {}", self.precision_bits)?;
        }

        Ok(())
    }
}

impl fmt::Display for meshtastic::User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "👤 User Profile:")?;
        writeln!(
            f,
            "  🆔 [{}] {:?} ({:?})",
            self.id, self.long_name, self.short_name
        )?;

        writeln!(
            f,
            "  🛠️ Hardware Model: {}",
            meshtastic::HardwareModel::try_from(self.hw_model)
                .unwrap()
                .as_str_name()
        )?;
        if self.is_licensed {
            writeln!(
                f,
                "  📡 Licensed Operator: {}",
                if self.is_licensed { "yes" } else { "no" }
            )?;
        }
        writeln!(
            f,
            "  🎭 Role: {}",
            meshtastic::config::device_config::Role::try_from(self.role)
                .unwrap()
                .as_str_name()
        )?;

        writeln!(f, "  🔐 Public Key: {} bytes", self.public_key.len())?;

        if let Some(unmessagable) = self.is_unmessagable {
            writeln!(
                f,
                "  🚫 Unmessagable: {}",
                if unmessagable { "yes" } else { "no" }
            )?;
        }

        Ok(())
    }
}

impl fmt::Display for meshtastic::NodeInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "🧭 Node #{} [!{:x}]:", self.num, self.num)?;

        if let Some(user) = &self.user {
            writeln!(f, "  {}", user)?; // assumes fmt::Display for User
        }

        if let Some(position) = &self.position {
            writeln!(f, "{}", position)?; // assumes fmt::Display for Position
        }

        writeln!(f, "  📶 SNR: {:.1} dB", self.snr)?;

        let ts = self.last_heard as i64;
        if let chrono::offset::LocalResult::Single(dt) = Utc.timestamp_opt(ts, 0) {
            writeln!(f, "  🕓 Last Heard: {}", dt.format("%Y-%m-%d %H:%M:%S UTC"))?;
        }

        if let Some(dm) = &self.device_metrics {
            writeln!(f, "{}", dm)?; // assumes fmt::Display for DeviceMetrics
        }

        if self.channel != 0 {
            writeln!(f, "  🔁 Channel Index: {}", self.channel)?;
        }

        if self.via_mqtt {
            writeln!(
                f,
                "  📡 Seen via MQTT: {}",
                if self.via_mqtt { "yes" } else { "no" }
            )?;
        }

        if let Some(hops) = self.hops_away {
            writeln!(f, "  🔀 Hops Away: {}", hops)?;
        }

        if self.is_favorite || self.is_ignored || self.is_key_manually_verified {
            writeln!(
                f,
                " {}{}{}",
                if self.is_favorite {
                    " ⭐️ Favorited"
                } else {
                    ""
                },
                if self.is_ignored { " 🚫 Ignored" } else { "" },
                if self.is_key_manually_verified {
                    "🔐 Key Verified"
                } else {
                    ""
                }
            )?;
        }

        Ok(())
    }
}

impl fmt::Display for meshtastic::NeighborInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "🌐 Neighbor Info for Node !{:x}, Last Sent By: !{:x}, Broadcast Interval: {} s",
            self.node_id, self.last_sent_by_id, self.node_broadcast_interval_secs
        )?;

        if self.neighbors.is_empty() {
            writeln!(f, "  🚫 No neighbors reported")?;
        } else {
            writeln!(f, "  👥 Neighbors [{}]:", self.neighbors.len())?;
            for neighbor in &self.neighbors {
                writeln!(f, "    ➖ {}", neighbor)?;
            }
        }

        Ok(())
    }
}

impl fmt::Display for meshtastic::Neighbor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let time_display = match Utc.timestamp_opt(self.last_rx_time as i64, 0) {
            chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            _ => String::from("<unknown>"),
        };

        writeln!(
            f,
            "Neighbor !{:x} SNR: {:.1} dB, Last Heard: {}, Broadcast Interval: {}",
            self.node_id, self.snr, time_display, self.node_broadcast_interval_secs
        )?;
        Ok(())
    }
}

impl fmt::Display for meshtastic::AdminMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "🛠 AdminMessage")?;

        // session_passkey в base64
        if self.session_passkey.is_empty() {
            writeln!(f, "  Session Passkey: <none>")?;
        } else {
            let b64 = general_purpose::STANDARD.encode(&self.session_passkey);
            writeln!(f, "  Session Passkey: {}", b64)?;
        }

        // payload_variant
        match &self.payload_variant {
            Some(variant) => {
                writeln!(f, "  Payload Variant:")?;
                match variant {
                    meshtastic::admin_message::PayloadVariant::GetConfigResponse(config) => {
                        writeln!(f, "{}", config)?
                    }

                    v => writeln!(f, "    {:?}", v)?,
                }
            }
            None => writeln!(f, "  Payload Variant: <none>")?,
        }

        Ok(())
    }
}

impl fmt::Display for meshtastic::Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "⚙️ Config")?;

        match &self.payload_variant {
            Some(variant) => {
                writeln!(f, "  Payload Variant:")?;
                match variant {
                    meshtastic::config::PayloadVariant::Security(security_config) => {
                        writeln!(f, "{}", security_config)?
                    }

                    v => writeln!(f, "{:?}", v)?,
                }
            }
            None => writeln!(f, "  Payload Variant: <none>")?,
        }

        Ok(())
    }
}

impl fmt::Display for meshtastic::config::SecurityConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "🔐 SecurityConfig")?;

        if self.public_key.is_empty() {
            writeln!(f, "  Public Key: <none>")?;
        } else {
            let b64 = general_purpose::STANDARD.encode(&self.public_key);
            writeln!(f, "  Public Key: {}", b64)?;
        }

        if self.private_key.is_empty() {
            writeln!(f, "  Private Key: <none>")?;
        } else {
            let b64 = general_purpose::STANDARD.encode(&self.private_key);
            writeln!(f, "  Private Key: {}", b64)?;
        }

        if self.admin_key.is_empty() {
            writeln!(f, "  Admin Keys: <none>")?;
        } else {
            writeln!(f, "  Admin Keys:")?;
            for (i, key) in self.admin_key.iter().enumerate() {
                let b64 = general_purpose::STANDARD.encode(key);
                writeln!(f, "    [{}]: {}", i, b64)?;
            }
        }

        writeln!(f, "  Is Managed: {}", self.is_managed)?;
        writeln!(f, "  Serial Enabled: {}", self.serial_enabled)?;
        writeln!(f, "  Debug Log API Enabled: {}", self.debug_log_api_enabled)?;
        writeln!(f, "  Admin Channel Enabled: {}", self.admin_channel_enabled)?;

        Ok(())
    }
}
