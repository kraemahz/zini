pub mod flows;
pub mod projects;
pub mod prompts;
pub mod jobs;
pub mod socket;
pub mod tasks;
pub mod users;
pub mod voice;

use tokio::sync::mpsc;
use warp::Filter;

pub const PAGE_SIZE: u32 = 20;

pub fn with_channel<M: Send + Sync>(
    channel: mpsc::Sender<M>,
) -> impl Filter<Extract = (mpsc::Sender<M>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || channel.clone())
}
