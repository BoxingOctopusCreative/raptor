use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct GlobalOpts {
    pub yes: bool,
    pub dry_run: bool,
    pub config: Option<PathBuf>,
}

impl GlobalOpts {
    pub fn apply(&self) {
        if let Some(ref path) = self.config {
            std::env::set_var("RAPTOR_CONFIG", path);
        }
    }
}
