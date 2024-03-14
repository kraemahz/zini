use clap::{Parser, Subcommand};
use diesel::prelude::*;
use diesel::result::QueryResult;
use http::Uri;
use prism_client::Client;
use subseq_util::tables::UserTable;
use uuid::Uuid;

use zini::api::users::StoredUserMeta;
use zini::events::{prism_url, USER_CREATED_BEAM, PROJECT_CREATED_BEAM};
use zini::tables::*;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short = 'd', long)]
    database: String,
    #[arg(short = 'p', long)]
    prism: Option<String>,
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init,
    User {
        user_id: Uuid,
        user_email: String,
        username: String,
        job_title: String
    },
    Project {
        user_id: Uuid,
        project_id: Uuid,
        project_name: String,
        project_description: String
    },
}

fn prism_client(addr: String) -> Client {
    let url = prism_url(&addr, None);
    let uri = url.parse::<Uri>().expect("is valid uri");
    Client::connect(uri, move |_| Ok(()))
}

fn main() -> QueryResult<()> {
    let args = Args::parse();
    let mut conn = PgConnection::establish(&args.database)
        .unwrap_or_else(|_| panic!("Failed to connect to database: {}", args.database));

    let prism = if let Some(prism_addr) = args.prism {
        Some(prism_client(prism_addr))
    } else {
        None
    };

    match args.cmd {
        Commands::Init => {
            seed_init_data(&mut conn)?;
        }
        Commands::User{user_id, user_email, username, job_title} => {
            let user = seed_user_data(&mut conn, user_id, user_email, username, job_title)?;
            println!("Created User: {}", serde_json::to_string(&user).unwrap());
            if let Some(mut prism) = prism {
                let vec = serde_json::to_vec(&user).unwrap();
                prism.emit(USER_CREATED_BEAM, vec).expect("prism user");
            }
        }
        Commands::Project { user_id, project_id, project_name, project_description } => {
            let project = seed_project_data(&mut conn, user_id, project_id, project_name, project_description)?;
            println!("Created Project: {}", serde_json::to_string(&project).unwrap());
            if let Some(mut prism) = prism {
                let vec = serde_json::to_vec(&project).unwrap();
                prism.emit(PROJECT_CREATED_BEAM, vec).expect("prism project");
            }
        }
    }
    Ok(())
}

fn seed_init_data(conn: &mut PgConnection) -> QueryResult<()> {
    let system_user = seed_user_data(conn,
                                     Uuid::nil(),
                                     "support@subseq.io".to_string(),
                                     "SUBSEQ".to_string(),
                                     "System".to_string())?;

    let entry_node = FlowNode::create(conn, "OPEN")?;
    let exit_node = FlowNode::create(conn, "CLOSED")?;
    let exits = vec![&exit_node];
    let graph = vec![(&entry_node, &exit_node)];

    let flow = Flow::create(
        conn,
        &system_user,
        "Default".to_string(),
        "This is the default flow".to_string(),
        &entry_node,
        graph,
        exits,
    )?;

    println!("Created Flow: {}", serde_json::to_string(&flow).unwrap());
    println!("Entry Node: {}", serde_json::to_string(&entry_node).unwrap());
    println!("Exit Node: {}", serde_json::to_string(&exit_node).unwrap());
    Ok(())
}

fn seed_user_data(conn: &mut PgConnection,
                  user_id: Uuid,
                  user_email: String,
                  username: String,
                  job_title: String) -> QueryResult<User> {
    let user = match User::from_email(conn, &user_email) {
        Some(user) => user,
        None => User::create(conn, user_id, &user_email, None)?,
    };
    let data = StoredUserMeta{job_title};
    UserIdAccount::create(conn, user_id, username, None).ok();
    UserMetadata::create(conn, user_id,
                         serde_json::to_value(&data).expect("serde")).ok();

    Ok(user)
}

fn seed_project_data(conn: &mut PgConnection,
                     user_id: Uuid,
                     project_id: Uuid,
                     project_name: String,
                     project_description: String) -> QueryResult<Project> {
    let user =  User::get(conn, user_id).expect("is valid user");
    let default_flow = Flow::default_flow(conn).expect("default flow exists");
    Project::create(
        conn,
        project_id,
        &user,
        &project_name,
        &project_description,
        &default_flow
    )
}
