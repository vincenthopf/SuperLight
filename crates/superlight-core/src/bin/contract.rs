use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use superlight_core::{config, devices, hidpp};

fn number(value: &Value, key: &str) -> i64 {
    value[key].as_i64().unwrap_or(0)
}
fn bytes(value: &Value) -> Vec<u8> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|n| n.as_u64().unwrap_or(0) as u8)
        .collect()
}

fn evaluate(value: Value) -> Result<Value, String> {
    Ok(match value["op"].as_str().ok_or("Missing operation")? {
        "encode" => json!(
            hidpp::encode(
                number(&value, "device") as u8,
                number(&value, "feature") as u8,
                number(&value, "function") as u8,
                &bytes(&value["params"])
            )
            .map_err(|e| e.to_string())?
        ),
        "parse" => {
            let raw = bytes(&value["raw"]);
            hidpp::parse(&raw).map_or(Value::Null, |m| {
                json!([m.device, m.feature, m.function, m.software, m.params])
            })
        }
        "smart_write" => {
            let mode = if value["mode"] == "freespin" {
                hidpp::ScrollMode::Freespin
            } else {
                hidpp::ScrollMode::Ratchet
            };
            json!({"feature": 7, "function": if value["enhanced"].as_bool().unwrap_or(false) { 2 } else { 1 }, "params": hidpp::SmartShift::parameters(mode, value["enabled"].as_bool().unwrap_or(false), number(&value, "threshold"))})
        }
        "smart_read" => json!(hidpp::SmartShift::decode(
            number(&value, "mode") as u8,
            number(&value, "threshold") as u8
        )),
        "dpi" => json!(devices::clamp_dpi(
            number(&value, "value"),
            devices::resolve(number(&value, "pid") as u16, "")
        )),
        "device" => devices::resolve(
            number(&value, "pid") as u16,
            value["name"].as_str().unwrap_or(""),
        )
        .map_or(
            Value::Null,
            |d| json!({"key": d.key, "name": d.name, "min": d.dpi_min, "max": d.dpi_max}),
        ),
        "candidates" => {
            let controls = serde_json::from_value::<Vec<hidpp::Control>>(value["controls"].clone())
                .map_err(|e| e.to_string())?;
            json!(hidpp::gesture_candidates(&controls, &hidpp::GESTURE_CIDS))
        }
        "migrate" => config::migrate(value["config"].clone())?,
        op => return Err(format!("Unknown operation: {op}")),
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = line?;
        if line.len() > superlight_core::CONFIG_LIMIT {
            return Err("Request too large".into());
        }
        let value = evaluate(serde_json::from_str(&line)?)?;
        serde_json::to_writer(&mut stdout, &value)?;
        writeln!(stdout)?;
    }
    stdout.flush()?;
    Ok(())
}
