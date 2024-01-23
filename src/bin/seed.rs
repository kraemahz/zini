use clap::Parser;
use diesel::prelude::*;
use diesel::result::QueryResult;
use subseq_util::tables::UserTable;
use uuid::Uuid;
use zini::tables::*;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(long)]
    database: String,
    base_user_id: Uuid,
    base_user_email: String
}

fn main() -> QueryResult<()> {
    let args = Args::parse();
    let mut conn = PgConnection::establish(&args.database)
        .expect(&format!("Failed to connect to database: {}", args.database));

    seed_data(&mut conn, args.base_user_id, args.base_user_email)
}

fn seed_data(conn: &mut PgConnection, user_id: Uuid, user_email: String) -> QueryResult<()> {
    if User::get(conn, user_id).is_some() {
        println!("Base user exists, exiting");
        return Ok(());
    }

    let user = User::create(conn, user_id, &user_email, None)?;

    let entry_node = FlowNode::create(conn, "OPEN")?;
    let exit_node = FlowNode::create(conn, "CLOSED")?;
    let exits = vec![&exit_node];
    let graph = vec![(&entry_node, &exit_node)];

    let flow = Flow::create(conn,
                            &user,
                            "Default".to_string(), 
                            "This is the default flow".to_string(),
                            &entry_node,
                            graph,
                            exits)?;


    println!("Created User: {}", serde_json::to_string(&user).unwrap());
    println!("Created Flow: {}", serde_json::to_string(&flow).unwrap());
    println!("Entry Node: {}", serde_json::to_string(&entry_node).unwrap());
    println!("Exit Node: {}", serde_json::to_string(&exit_node).unwrap());

    Ok(())
}
