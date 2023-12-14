use chrono::NaiveDateTime;
use diesel::{prelude::*, pg::Pg, connection::LoadConnection};
use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::User;


#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::flows)]
pub struct Flow {
    id: Uuid,
    owner_id: Uuid,
    created: NaiveDateTime,
    flow_name: String,
    description: String
}

impl Flow {
    pub fn create<C>(conn: &mut C,
                     sender: &mut broadcast::Sender<Self>,
                     author: &User,
                     flow_name: String,
                     description: String) -> QueryResult<Self>
        where C: Connection<Backend = Pg>
    {
        let flow = Self {
            id: Uuid::new_v4(),
            owner_id: author.id,
            created: chrono::Utc::now().naive_utc(),
            flow_name: flow_name.to_ascii_uppercase(),
            description };
        diesel::insert_into(crate::schema::flows::table)
            .values(&flow)
            .execute(conn)?;
        sender.send(flow.clone()).ok();
        Ok(flow)
    }

    pub fn get<C>(conn: &mut C, flow_id: Uuid) -> Option<Self>
        where C: Connection<Backend = Pg> + LoadConnection
    {
        use crate::schema::flows::dsl;
        let task = dsl::flows.find(flow_id)
            .get_result::<Flow>(conn)
            .optional()
            .ok()??;
        Some(task)
    }
}

#[derive(Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::flow_nodes)]
pub struct FlowNode {
    pub id: Uuid,
    pub node_name: String
}

impl FlowNode {
    pub fn get<C>(conn: &mut C, node_id: Uuid) -> Option<Self>
        where C: Connection<Backend = Pg> + LoadConnection
    {
        use crate::schema::flow_nodes::dsl;
        let flow_node = dsl::flow_nodes.find(node_id)
            .get_result::<FlowNode>(conn)
            .optional()
            .ok()??;
        Some(flow_node)
    }

    pub fn create<C>(conn: &mut C, node_name: String) -> QueryResult<Self>
        where C: Connection<Backend = Pg>
    {
        let id = Uuid::new_v4();
        let new_flow_node = FlowNode{id, node_name};
        diesel::insert_into(crate::schema::flow_nodes::table)
            .values(&new_flow_node)
            .execute(conn)?;
        Ok(new_flow_node)
    }
}

#[derive(Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::flow_node_connections)]
pub struct FlowConnection {
    from_node_id: Uuid,
    to_node_id: Uuid 
}

impl FlowConnection {
    pub fn connect_all<C>(conn: &mut C, flow_id: Uuid, nodes: Vec<(FlowNode, FlowNode)>) -> QueryResult<()>
        where C: Connection<Backend = Pg> + LoadConnection
    {
        // 1. Get all nodes in this flow from FlowAssignment
        let node_ids: Vec<Uuid> = crate::schema::flow_assignments::table
            .filter(crate::schema::flow_assignments::flow_id.eq(flow_id))
            .select(crate::schema::flow_assignments::node_id)
            .load::<Uuid>(conn)?;

        // 2. Delete all FlowConnections on those nodes, but only within this flow
        diesel::delete(
            crate::schema::flow_node_connections::table
                .filter(
                    crate::schema::flow_node_connections::from_node_id.eq_any(&node_ids)
                    .and(crate::schema::flow_node_connections::to_node_id.eq_any(&node_ids))
                )
        ).execute(conn)?;

        // 3. Create all new connections from nodes
        for (from_node, to_node) in nodes {
            let new_connection = FlowConnection {
                from_node_id: from_node.id,
                to_node_id: to_node.id,
            };
            diesel::insert_into(crate::schema::flow_node_connections::table)
                .values(&new_connection)
                .execute(conn)?;
        }

        Ok(())
    }
}

#[derive(Queryable, Insertable, Clone, Debug)]
#[diesel(table_name = crate::schema::flow_assignments)]
pub struct FlowAssignment {
    flow_id: Uuid,
    node_id: Uuid,
}

impl FlowAssignment {
    pub fn assign<C>(conn: &mut C, flow_id: Uuid, flow_node: &FlowNode) -> QueryResult<()>
        where C: Connection<Backend = Pg>
    {
        let assignment = Self { flow_id, node_id: flow_node.id };
        diesel::insert_into(crate::schema::flow_assignments::table)
            .values(&assignment)
            .execute(conn)?;
        Ok(())
    }
}


#[derive(Queryable, Insertable, Clone, Debug)]
#[diesel(table_name = crate::schema::flow_entries)]
pub struct FlowEntry {
    flow_id: Uuid,
    node_id: Uuid,
}

impl FlowEntry {
    pub fn set_flow_entry<C>(conn: &mut C, flow_id: Uuid, node_id: Uuid) -> QueryResult<()>
        where C: Connection<Backend = Pg>
    {
        let entry = FlowEntry{flow_id, node_id};
        diesel::insert_into(crate::schema::flow_entries::table)
            .values(&entry)
            .on_conflict(crate::schema::flow_entries::flow_id)
            .do_update()
            .set(crate::schema::flow_entries::node_id.eq(node_id))
            .execute(conn)?;
        Ok(())
    }
}

#[derive(Queryable, Insertable, Clone, Debug)]
#[diesel(table_name = crate::schema::flow_exits)]
pub struct FlowExit {
    flow_id: Uuid,
    node_id: Uuid,
}

impl FlowExit {
    pub fn clear_and_set<C>(conn: &mut C, flow_id: Uuid, exit_nodes: Vec<FlowNode>) -> QueryResult<()>
        where C: Connection<Backend = Pg>
    {
        diesel::delete(crate::schema::flow_exits::table)
            .filter(crate::schema::flow_exits::flow_id.eq(flow_id))
            .execute(conn)?;
        for node in exit_nodes {
            let new_exit = FlowExit{flow_id, node_id: node.id};
            diesel::insert_into(crate::schema::flow_exits::table)
                .values(&new_exit)
                .execute(conn)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Graph {
    flow_id: Uuid,
    entry_point: FlowNode,
    exit_points: Vec<FlowNode>,
    nodes: Vec<FlowNode>,
    connections: Vec<FlowConnection>,
}

impl Graph {
    pub fn fetch<C>(conn: &mut C, flow_id: Uuid) -> QueryResult<Self>
    where
        C: Connection<Backend = Pg> + LoadConnection
    {
        // Fetch entry point
        let entry_point_id = crate::schema::flow_entries::table
            .filter(crate::schema::flow_entries::flow_id.eq(flow_id))
            .select(crate::schema::flow_entries::node_id)
            .first::<Uuid>(conn)?;

        let entry_point = crate::schema::flow_nodes::table
            .find(entry_point_id)
            .first::<FlowNode>(conn)?;

        // Fetch exit points
        let exit_point_ids = crate::schema::flow_exits::table
            .filter(crate::schema::flow_exits::flow_id.eq(flow_id))
            .select(crate::schema::flow_exits::node_id)
            .load::<Uuid>(conn)?;

        let exit_points = crate::schema::flow_nodes::table
            .filter(crate::schema::flow_nodes::id.eq_any(exit_point_ids))
            .load::<FlowNode>(conn)?;

        // Fetch all nodes in the flow
        let node_ids = crate::schema::flow_assignments::table
            .filter(crate::schema::flow_assignments::flow_id.eq(flow_id))
            .select(crate::schema::flow_assignments::node_id)
            .load::<Uuid>(conn)?;

        let nodes = crate::schema::flow_nodes::table
            .filter(crate::schema::flow_nodes::id.eq_any(&node_ids))
            .load::<FlowNode>(conn)?;

        // Fetch all connections in the flow
        let connections = crate::schema::flow_node_connections::table
            .filter(
                crate::schema::flow_node_connections::from_node_id.eq_any(&node_ids)
                .or(crate::schema::flow_node_connections::to_node_id.eq_any(&node_ids))
            )
            .load::<FlowConnection>(conn)?;

        Ok(Graph {
            flow_id,
            entry_point,
            exit_points,
            nodes,
            connections,
        })
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use crate::tables::harness::{to_pg_db_name, DbHarness};
    use function_name::named;

    #[test]
    #[named]
    fn test_flow_handle() {
        let db_name = to_pg_db_name(function_name!());
        let harness = DbHarness::new("localhost", "development", &db_name);
        let mut conn = harness.conn(); 
        let (mut tx, _) = broadcast::channel(1);
        let (mut user_tx, _) = broadcast::channel(1);

        let user = User::create(&mut conn, &mut user_tx, "test@example.com", None, None).expect("user");
        let flow = Flow::create(&mut conn, &mut tx, &user, "flow".to_string(), "".to_string()).expect("flow");
        let flow2 = Flow::get(&mut conn, flow.id).expect("task2");

        assert_eq!(flow, flow2);
        assert_eq!(flow.flow_name, "FLOW"); // Forced uppercase
    }
}
