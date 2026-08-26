use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::RwLock;

const ADJ: &[&str] = &[
    "琥珀", "夜航", "量子", "薄荷", "远山", "星尘", "疾风", "深空", "微光", "极昼",
];
const NOUN: &[&str] = &[
    "猎豹", "信标", "旅人", "灯塔", "狐狸", "信天翁", "松果", "鲸鱼", "萤火", "雪松",
];

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub nickname: String,
    pub avatar_ver: u64,
    pub theme: String,
    pub hotkey: String,
    pub close_to_tray: bool,
    pub onboarded: bool,
    pub data_dir: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        let mut rng = rand::thread_rng();
        Config {
            nickname: format!(
                "{}{}#{:04}",
                ADJ.choose(&mut rng).unwrap(),
                NOUN.choose(&mut rng).unwrap(),
                rand::Rng::gen_range(&mut rng, 0..10000)
            ),
            avatar_ver: 1,
            theme: "dark".into(),
            hotkey: "Ctrl+Alt+S".into(),
            close_to_tray: true,
            onboarded: false,
            data_dir: None,
        }
    }
}

impl Config {
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let path = dir.join("config.json");
        let data = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, data).map_err(|e| e.to_string())
    }
}

pub fn load_or_create(dir: &Path) -> Result<RwLock<Config>, String> {
    let path = dir.join("config.json");
    let cfg = if path.exists() {
        let raw = std::fs::read(&path).map_err(|e| e.to_string())?;
        serde_json::from_slice::<Config>(&raw).unwrap_or_default()
    } else {
        Config::default()
    };
    cfg.save(dir)?;
    Ok(RwLock::new(cfg))
}
