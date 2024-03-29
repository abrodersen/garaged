
use std::time::Duration;
use std::str::{from_utf8, FromStr};

use strum::{EnumString, Display};
use sysfs_gpio::{Direction, Edge, Pin};

use rumqttc::{MqttOptions, AsyncClient, QoS, Event, Incoming};

use serde_json::{json, to_vec};

use tokio::time::{sleep, interval};
use tokio::sync::Mutex;

use futures::StreamExt;

use anyhow::{Error, Context};

#[derive(Debug, PartialEq, Display, EnumString)]
enum Status {
    #[strum(serialize = "open")]
    Open,
    #[strum(serialize = "closed")]
    Closed,
}

#[derive(Debug, PartialEq, Display, EnumString)]
enum Command {
    #[strum(serialize = "OPEN")]
    Open,
    #[strum(serialize = "CLOSE")]
    Close,
}

struct Hardware {
    led: Option<Pin>,
    relay: Pin,
    status: Pin,
    input: Pin,
    lock: Mutex<()>,
}

impl Hardware {
    fn init(enable_led: bool) -> Result<Hardware, Error> {
        let led_pin = if enable_led {
            println!("initalizing led pin");
            let led_pin = Pin::new(7);
            led_pin.export()?;
            led_pin.set_direction(Direction::Low)?;
            Some(led_pin)
        } else {
            None
        };

        println!("initalizing relay pin");
        let relay_pin = Pin::new(17);
        relay_pin.export()?;
        relay_pin.set_direction(Direction::Low)?;

        println!("initalizing status pin");
        let status_pin = Pin::new(6);
        status_pin.export()?;
        status_pin.set_direction(Direction::In)?;
        status_pin.set_edge(Edge::BothEdges)?;

        println!("initalizing input pin");
        let input_pin = Pin::new(12);
        input_pin.export()?;
        input_pin.set_direction(Direction::In)?;
        input_pin.set_edge(Edge::RisingEdge)?;

        Ok(Hardware {
            led: led_pin,
            relay: relay_pin,
            status: status_pin,
            input: input_pin,
            lock: Mutex::new(()),
        })
    }
}

impl Drop for Hardware {
    fn drop(&mut self) {
        if let Some(led) = self.led {
            let _ = led.unexport();
        }
        let _ = self.relay.unexport();
        let _ = self.status.unexport();
        let _ = self.input.unexport();
    }
}

fn get_door_status(hw: &Hardware) -> Result<Status, Error> {
    hw.status.get_value()
        .map(parse_door_status)
        .map_err(Error::from)
}

fn parse_door_status(status: u8) -> Status {
    match status {
        0 => Status::Open,
        _ => Status::Closed,
    }
}

async fn trigger_relay(hw: &Hardware) -> Result<(), Error> {
    let _ = hw.lock.lock().await;
    println!("triggering door relay");
    if let Some(led) = hw.led {
        led.set_value(1)?;
    }
    hw.relay.set_value(1)?;
    sleep(Duration::from_millis(200)).await;
    hw.relay.set_value(0)?;
    if let Some(led) = hw.led {
        led.set_value(0)?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error>  {
    println!("initializing gpio");
    let hw = Hardware::init(false)?;
    let mut status_changes = hw.status.get_value_stream()?;
    let mut input_triggers = hw.input.get_value_stream()?;

    println!("initializing mqtt");
    let hostname = gethostname::gethostname().into_string().expect("failed to get hostname");
    let mut options = MqttOptions::new(hostname, "10.44.0.15", 1883);
    options.set_keep_alive(Duration::from_secs(5));

    let mqtt_path = "homeassistant/cover/garage";
    let config_topic = format!("{}/config", mqtt_path);
    let command_topic = format!("{}/command", mqtt_path);
    let state_topic = format!("{}/state", mqtt_path);

    let (client, mut event_loop) = AsyncClient::new(options, 10);
    let config = json!({
        "name": "Garage",
        "unique_id": "garage_door",
        "command_topic": command_topic,
        "payload_close": Command::Close.to_string(),
        "payload_open": Command::Open.to_string(),
        "state_topic": state_topic,
        "state_open": Status::Open.to_string(),
        "state_closed": Status::Closed.to_string(),
        "device_class": "garage",
    });
    println!("publishing device config");
    client.publish(config_topic, QoS::AtLeastOnce, false, to_vec(&config)?).await?;
    client.subscribe(&command_topic, QoS::ExactlyOnce).await?;

    println!("publishing initial door state");
    let status = get_door_status(&hw)?;
    println!("initial door state = {}", status);
    client.publish(&state_topic, QoS::AtLeastOnce, true, status.to_string()).await?;

    let mut timer = interval(Duration::from_secs(60));

    println!("beginning monitor loop");
    loop {
        tokio::select! {
            _next_timer = timer.tick() => {
                let status = get_door_status(&hw)?;
                client.publish(&state_topic, QoS::AtLeastOnce, true, status.to_string()).await?;
            },
            next_status = status_changes.next() => {
                match next_status {
                    Some(Ok(x)) => {
                        let status = parse_door_status(x);
                        println!("detected door status = {}", status);
                        client.publish(&state_topic, QoS::AtLeastOnce, true, status.to_string()).await?;
                    },
                    Some(Err(e)) => return Err(e).context("error reading door status events"),
                    None => break,
                }
            },
            next_input = input_triggers.next() => {
                match next_input {
                    Some(Ok(x)) if x != 0 => {
                        println!("detected input trigger");
                        trigger_relay(&hw).await?;
                    },
                    Some(Ok(_)) => (),
                    Some(Err(e)) => return Err(e).context("error reading input trigger events"),
                    None => break,
                }
            },
            next_msg = event_loop.poll() => {
                match next_msg.context("error reading mqtt events") {
                    Ok(Event::Incoming(Incoming::Publish(packet))) => {
                        if packet.topic == command_topic {
                            let command = from_utf8(packet.payload.as_ref())
                                .map_err(Error::from)
                                .and_then(|s| Command::from_str(s).map_err(Error::from));
                            let command = match command {
                                Ok(c) => c,
                                Err(_) => {
                                    println!("invalid payload on command topic");
                                    continue;
                                }
                            };
                            let current_status = get_door_status(&hw)?;
                            println!("command = {}, door status = {}", command, current_status);
                            match (command, current_status) {
                                (Command::Open, Status::Closed) |
                                (Command::Close, Status::Open) => {
                                    trigger_relay(&hw).await?;
                                },
                                _ => {
                                    println!("invalid command, ignoring");
                                }
                            }
                        } else {
                            println!("unrecognized topic {}", packet.topic);
                        }
                        
                    },
                    Err(e) => {
                        println!("mqtt error: {}", e);
                    }
                    _ => (),
                }
            },
            _ = tokio::signal::ctrl_c() => {
                println!("shutdown signal received");
                break;
            }
        }
    }

    println!("exiting program");
    Ok(())
}
