use std::sync::Arc;

use diesel::connection::{Connection, LoadConnection};
use diesel::pg::Pg;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use subseq_util::{api::*, Router};
use subseq_util::oidc::IdentityProvider;
use subseq_util::tables::{DbPool, UserTable};
use tokio::sync::broadcast;
use uuid::Uuid;
use warp::{Filter, Reply, Rejection};
use warp_sessions::MemoryStore;

use crate::tables::*;


#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct FlowNodePayload {
    name: String,
    id: Option<Uuid>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct NewFlowPayload {
    flow_name: String,
    description: Option<String>,
    entry: FlowNodePayload,
    exits: Vec<FlowNodePayload>,
    connections: Vec<(FlowNodePayload, FlowNodePayload)>
}

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct UpdateFlowPayload {
    flow_id: Uuid,
    entry: FlowNodePayload,
    exits: Vec<FlowNodePayload>,
    connections: Vec<(FlowNodePayload, FlowNodePayload)>
}


fn find_node<'a>(nodes: &[&'a FlowNode], node_to_find: FlowNodePayload) -> Option<&'a FlowNode> {
    for node_from in nodes.iter() {
        if let Some(id) = &node_to_find.id {
            if &node_from.id == id {
                return Some(node_from);
            }
        } else if node_to_find.name == node_from.node_name {
            return Some(node_from);
        }
    }
    None
}


// Handler for creating a new flow
async fn create_flow_handler(
    payload: NewFlowPayload,
    auth: AuthenticatedUser,
    db_pool: Arc<DbPool>,
    sender: broadcast::Sender<Flow>
) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };

    let NewFlowPayload{flow_name, description, entry, exits, connections} = payload;

    let mut nodes: Vec<(FlowNode, FlowNode)> = vec![];
    for (node_from, node_to) in connections.into_iter() {
        let source = match create_or_get_flow_node(&mut conn, node_from) {
            Ok(s) => s,
            Err(_) => return Err(warp::reject::custom(DatabaseError{}))
        };
        let sink = match create_or_get_flow_node(&mut conn, node_to) {
            Ok(s) => s,
            Err(_) => return Err(warp::reject::custom(DatabaseError{}))
        };
        nodes.push((source, sink));
    }
    let mut sources: Vec<&FlowNode> = vec![];
    let mut sinks: Vec<&FlowNode> = vec![];
    let mut graph: Vec<(&FlowNode, &FlowNode)> = vec![];

    for (source, sink) in nodes.iter() {
        sources.push(source);
        sinks.push(sink);
        graph.push((source, sink));
    }

    let entry: &FlowNode = match find_node(&sources, entry) {
        Some(entry) => entry,
        None => return Err(warp::reject::custom(InvalidConfigurationError{}))
    };
    let mut exit_refs = vec![];
    for exit in exits {
        match find_node(&sinks, exit) {
            Some(exit) => exit_refs.push(exit),
            None => return Err(warp::reject::custom(InvalidConfigurationError{}))
        }
    }

    let user = match User::get(&mut conn, auth.id()) {
        Some(user) => user,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };

    let flow = match Flow::create(
        &mut conn,
        &user,
        flow_name,
        description.unwrap_or_else(String::new),
        &entry,
        graph,
        exit_refs
    ) {
        Ok(flow) => flow,
        Err(_) => return Err(warp::reject::custom(ConflictError{}))
    };
    sender.send(flow.clone()).ok();

    // Create a response with the created flow details
    let reply = warp::reply::json(&flow);
    Ok(reply)
}

fn create_or_get_flow_node<C>(conn: &mut C, node: FlowNodePayload) -> QueryResult<FlowNode>
    where C: Connection<Backend = Pg> + LoadConnection
{
    let flow_node = match node.id {
        Some(id) => match FlowNode::get(conn, id) {
            Some(node) => node,
            None => return Err(diesel::result::Error::NotFound)
        },
        None => FlowNode::create(conn, &node.name)?
    };
    Ok(flow_node)
}


async fn get_flow_graph_handler(
    flow_id: String,
    _auth: AuthenticatedUser,
    db_pool: Arc<DbPool>,
) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let flow_id = Uuid::parse_str(&flow_id)
        .map_err(|_| warp::reject::custom(ParseError{}))?;

    let graph = Graph::fetch(&mut conn, flow_id).map_err(|_| {
        warp::reject::custom(DatabaseError{})
    })?;
    let reply = warp::reply::json(&graph);
    Ok(reply)
}

const NUM_FLOWS_PER_PAGE: u32 = 10;

async fn list_flows_handler(page: u32, _auth: AuthenticatedUser, db_pool: Arc<DbPool>) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let flows = Flow::list(&mut conn, page, NUM_FLOWS_PER_PAGE);
    Ok(warp::reply::json(&flows))
}

async fn update_flow_graph_handler(
    payload: UpdateFlowPayload,
    _auth: AuthenticatedUser,
    db_pool: Arc<DbPool>,
    sender: broadcast::Sender<Graph>
) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let UpdateFlowPayload{flow_id, entry, exits, connections} = payload;

    conn.transaction(|transaction| {
        let mut flow = match Flow::get(transaction, flow_id) {
            Some(f) => f,
            None => return Err(diesel::result::Error::NotFound)
        };
        // Set Entry Node
        let entry = create_or_get_flow_node(transaction, entry)?;
        flow.set_flow_entry(transaction, &entry)?;

        // Set Exit Nodes
        let mut exit_nodes = vec![];
        for exit in exits {
            let exit_node = create_or_get_flow_node(transaction, exit)?;
            exit_nodes.push(exit_node);
        }
        let exit_refs: Vec<&FlowNode> = exit_nodes.iter().collect();
        FlowExit::create_exits(transaction, flow_id, exit_refs)?;

        // Set Edges
        let mut edges = vec![];
        for (edge_from, edge_to) in connections {
            let edge_from = create_or_get_flow_node(transaction, edge_from)?;
            let edge_to = create_or_get_flow_node(transaction, edge_to)?;
            edges.push((edge_from, edge_to));
        }
        let edge_refs: Vec<(&FlowNode, &FlowNode)> = edges
            .iter()
            .map(|(source, sink)| (source, sink))
            .collect();
        FlowConnection::connect_all(transaction, flow_id, edge_refs)?;

        Ok::<(), diesel::result::Error>(())
    }).map_err(|_| {
        warp::reject::custom(DatabaseError{})
    })?;
    let graph = Graph::fetch(&mut conn, flow_id).map_err(|_| {
        warp::reject::custom(DatabaseError{})
    })?;
    sender.send(graph.clone()).ok();
    let reply = warp::reply::json(&graph);
    Ok(reply)
}

// Add the route for creating a flow to your routes function
pub fn routes(idp: Option<Arc<IdentityProvider>>,
              session: MemoryStore,
              pool: Arc<DbPool>,
              router: &mut Router) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone
{
    let flow_tx: broadcast::Sender<Flow> = router.announce();
    let graph_tx: broadcast::Sender<Graph> = router.announce();

    let create_flow = warp::post()
        .and(warp::body::json())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(flow_tx))
        .and_then(create_flow_handler);

    let list_flows = warp::path("list")
        .and(warp::path::param())
        .and(warp::get())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(list_flows_handler);

    let get_flow_graph = warp::get()
        .and(warp::path::param())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(get_flow_graph_handler);

    let update_flow_graph = warp::put()
        .and(warp::path("graph"))
        .and(warp::body::json())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(graph_tx))
        .and_then(update_flow_graph_handler);

    warp::path("flow")
        .and(create_flow
             .or(list_flows)
             .or(get_flow_graph)
             .or(update_flow_graph))
}
