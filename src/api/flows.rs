use std::sync::Arc;
use diesel::connection::{Connection, LoadConnection};
use diesel::pg::Pg;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;
use warp::{Filter, Reply, Rejection};

use super::*;
use crate::router::with_broadcast;
use crate::tables::*;
use super::sessions::{AuthenticatedUser, SessionStore, authenticate};


#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct FlowNodePayload {
    name: String,
    id: Option<Uuid>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct NewFlowPayload {
    flow_name: String,
    description: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct UpdateFlowPayload {
    flow_id: Uuid,
    entry: FlowNodePayload,
    exits: Vec<FlowNodePayload>,
    connections: Vec<(FlowNodePayload, FlowNodePayload)>
}


// Handler for creating a new flow
async fn create_flow_handler(
    payload: NewFlowPayload,
    auth: AuthenticatedUser,
    db_pool: Arc<DbPool>,
    mut sender: broadcast::Sender<Flow>
) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };

    let NewFlowPayload{flow_name, description} = payload;
    let user = match User::get(&mut conn, auth.id()) {
        Some(user) => user,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };

    let flow = match Flow::create(
        &mut conn,
        &mut sender,
        &user,
        flow_name,
        description.unwrap_or_else(String::new)
    ) {
        Ok(flow) => flow,
        Err(_) => return Err(warp::reject::custom(ConflictError{}))
    };

    // Create a response with the created flow details
    let reply = warp::reply::json(&flow);
    Ok(reply)
}

fn create_or_get_flow_node<C>(conn: &mut C, node: FlowNodePayload, flow_id: Uuid) -> QueryResult<FlowNode>
    where C: Connection<Backend = Pg> + LoadConnection
{
    let flow_node = match node.id {
        Some(id) => match FlowNode::get(conn, id) {
            Some(node) => node,
            None => return Err(diesel::result::Error::NotFound)
        },
        None => FlowNode::create(conn, node.name)?
    };
    FlowAssignment::assign(conn, flow_id, &flow_node)?;
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
        // Set Entry Node
        let entry = create_or_get_flow_node(transaction, entry, flow_id)?;
        FlowEntry::set_flow_entry(transaction, flow_id, entry.id)?;

        // Set Exit Nodes
        let mut exit_nodes = vec![];
        for exit in exits {
            let exit_node = create_or_get_flow_node(transaction, exit, flow_id)?;
            exit_nodes.push(exit_node);
        }
        FlowExit::clear_and_set(transaction, flow_id, exit_nodes)?;

        // Set Edges
        let mut edges = vec![];
        for (edge_from, edge_to) in connections {
            let edge_from = create_or_get_flow_node(transaction, edge_from, flow_id)?;
            let edge_to = create_or_get_flow_node(transaction, edge_to, flow_id)?;
            edges.push((edge_from, edge_to));
        }
        FlowConnection::connect_all(transaction, flow_id, edges)?;

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
pub fn routes(store: Arc<SessionStore>,
              pool: Arc<DbPool>,
              flow_tx: broadcast::Sender<Flow>,
              graph_tx: broadcast::Sender<Graph>) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let create_flow = warp::post()
        .and(warp::body::json())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(flow_tx))
        .and_then(create_flow_handler);

    let get_flow_graph = warp::get()
        .and(warp::path::param())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and_then(get_flow_graph_handler);

    let update_flow_graph = warp::put()
        .and(warp::path("graph"))
        .and(warp::body::json())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(graph_tx))
        .and_then(update_flow_graph_handler);

    warp::path("flow")
        .and(create_flow
             .or(get_flow_graph)
             .or(update_flow_graph))
}
