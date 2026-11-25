//! Ringing this machine when a paired device pages it.

use std::collections::HashMap;
use std::time::Duration;

use continuum_core::identity::DeviceId;
use continuum_node::NodeHandle;
use continuum_node::features::page::{self, PageAction};
use tokio::process::Command;
use tokio::task::JoinHandle;

const SOUND: &str = "/usr/share/sounds/freedesktop/stereo/phone-incoming-call.oga";

#[derive(Default)]
pub struct Ringer {
    active: HashMap<DeviceId, JoinHandle<()>>,
}

impl Ringer {
    /// Rings until `duration`, a stop from the pager, or the user clicking
    /// "Found it" on the notification (which tells the pager).
    pub fn start(&mut self, peer: DeviceId, name: String, duration: Duration, node: NodeHandle) {
        self.stop(&peer);
        let task = tokio::spawn(async move {
            // Any interaction counts as "found": clicking the notification
            // (the default action; some daemons, like mako, show no
            // buttons), the "Found it" button, or dismissing it. Critical
            // notifications don't expire on their own.
            let notify = async {
                let _ = Command::new("notify-send")
                    .args(["--app-name=Continuum", "--urgency=critical", "--wait"])
                    .args([
                        "--action=default=Found it",
                        "--action=found=Found it",
                        "--icon=phone",
                    ])
                    .arg(format!("{name} is looking for this computer"))
                    .arg("Click to stop the sound.")
                    .kill_on_drop(true)
                    .output()
                    .await;
            };
            let sound = async {
                loop {
                    let played = Command::new("pw-play")
                        .arg(SOUND)
                        .kill_on_drop(true)
                        .status()
                        .await;
                    if played.is_err() {
                        // No PipeWire: try PulseAudio, then give up quietly.
                        let _ = Command::new("paplay")
                            .arg(SOUND)
                            .kill_on_drop(true)
                            .status()
                            .await;
                    }
                }
            };
            tokio::select! {
                () = notify => {
                    let _ = node.send_feature(peer, page::CAPABILITY, page::encode(PageAction::Found)).await;
                }
                () = sound => {}
                () = tokio::time::sleep(duration) => {}
            }
        });
        self.active.insert(peer, task);
    }

    pub fn stop(&mut self, peer: &DeviceId) {
        if let Some(task) = self.active.remove(peer) {
            task.abort();
        }
    }
}
