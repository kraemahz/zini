pub mod flows;
pub mod projects;
pub mod prompts;
pub mod socket;
pub mod tasks;
pub mod users;
pub mod voice;

use tokio::sync::mpsc;
use warp::Filter;

pub fn with_channel<M: Send + Sync>(channel: mpsc::Sender<M>)
    -> impl Filter<Extract = (mpsc::Sender<M>,), Error = std::convert::Infallible> + Clone
{
    warp::any().map(move || channel.clone())
}
