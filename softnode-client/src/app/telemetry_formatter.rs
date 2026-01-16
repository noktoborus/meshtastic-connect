use std::fmt;

use crate::app::data::TelemetryVariant;

#[derive(serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
}

impl fmt::Display for TemperatureUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TemperatureUnit::Celsius => write!(f, "°C"),
            TemperatureUnit::Fahrenheit => write!(f, "°F"),
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BarometricUnit {
    Hectopascals,
    MillimetersOfMercury,
}

impl fmt::Display for BarometricUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BarometricUnit::Hectopascals => write!(f, "hPa"),
            BarometricUnit::MillimetersOfMercury => write!(f, "mmHg"),
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TelemetryFormatter {
    pub temperature_units: TemperatureUnit,
    pub barometric_units: BarometricUnit,
}

impl Default for TelemetryFormatter {
    fn default() -> Self {
        Self {
            temperature_units: TemperatureUnit::Celsius,
            barometric_units: BarometricUnit::Hectopascals,
        }
    }
}

impl TelemetryFormatter {
    pub fn value(&self, value: f64, variant: TelemetryVariant) -> f64 {
        match variant {
            TelemetryVariant::BarometricPressure => match self.barometric_units {
                BarometricUnit::Hectopascals => value,
                BarometricUnit::MillimetersOfMercury => value * 0.750063755419211,
            },
            TelemetryVariant::EnvironmentTemperature => match self.temperature_units {
                TemperatureUnit::Celsius => value,
                TemperatureUnit::Fahrenheit => value * 1.8 + 32.0,
            },
            TelemetryVariant::Lux => value,
            TelemetryVariant::Iaq => value,
            TelemetryVariant::Humidity => value,
            TelemetryVariant::GasResistance => value,
            TelemetryVariant::Radiation => value,
            TelemetryVariant::PowerMetricVoltage(_) => value,
            TelemetryVariant::PowerMetricCurrent(_) => value,
            TelemetryVariant::AirUtilTx => value,
            TelemetryVariant::ChannelUtilization => value,
            TelemetryVariant::Voltage => value,
            TelemetryVariant::BatteryLevel => value,
            TelemetryVariant::HeartRate => value,
            TelemetryVariant::SpO2 => value,
            TelemetryVariant::HealthTemperature => value,
        }
    }
    pub fn format(&self, value: f64, variant: TelemetryVariant) -> String {
        let value = self.value(value, variant);
        match variant {
            TelemetryVariant::BarometricPressure => match self.barometric_units {
                BarometricUnit::Hectopascals => format!("{:.2} hPa", value),
                BarometricUnit::MillimetersOfMercury => {
                    format!("{:.2} mmHg", value)
                }
            },
            TelemetryVariant::EnvironmentTemperature => match self.temperature_units {
                TemperatureUnit::Celsius => {
                    format!("{:.2} °C", value)
                }
                TemperatureUnit::Fahrenheit => {
                    format!("{:.2} °F", value)
                }
            },
            TelemetryVariant::Lux => format!("{:.2} lx", value),
            TelemetryVariant::Iaq => format!("{:.2} IAQ", value),
            TelemetryVariant::Humidity => format!("{:.2}%", value),
            TelemetryVariant::GasResistance => format!("{:.2} kΩ", value),
            TelemetryVariant::Radiation => format!("{:.2} μSv/h", value),
            TelemetryVariant::PowerMetricVoltage(_) => format!("{:.2} V", value),
            TelemetryVariant::PowerMetricCurrent(_) => format!("{:.2} A", value),
            TelemetryVariant::AirUtilTx => format!("{:.2} %/min", value),
            TelemetryVariant::ChannelUtilization => format!("{:.2} %/min", value),
            TelemetryVariant::Voltage => format!("{:.2} V", value),
            TelemetryVariant::BatteryLevel => format!("{:.0}%", value),
            TelemetryVariant::HeartRate => format!("{:.2} bpm", value),
            TelemetryVariant::SpO2 => format!("{:.2}%", value),
            TelemetryVariant::HealthTemperature => format!("{:.2} °C", value),
        }
    }
}
