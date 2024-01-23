use chrono::NaiveDateTime;
use diesel::{prelude::*, pg::Pg, connection::LoadConnection};
use serde::Serialize;
use uuid::Uuid;

use super::*;


#[derive(Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::flows)]
pub struct Flow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub created: NaiveDateTime,
    pub flow_name: String,
    pub description: String,
    pub entry_node_id: Uuid
}

impl PartialEq for Flow {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id &&
            self.owner_id == other.owner_id &&
            self.created.timestamp_micros() == other.created.timestamp_micros() &&
            self.flow_name == other.flow_name &&
            self.description == other.description &&
            self.entry_node_id == other.entry_node_id
    }
}

impl Flow {
    pub fn create<C>(conn: &mut C,
                     author: &User,
                     flow_name: String,
                     description: String,
                     entry_node: &FlowNode,
                     graph: Vec<(&FlowNode, &FlowNode)>,
                     exits: Vec<&FlowNode>) -> QueryResult<Self>
        where C: Connection<Backend = Pg> + LoadConnection
    {
        let sinks: Vec<_> = graph.iter().map(|(_, &ref sink)| sink).collect();
        let sources: Vec<_> = graph.iter().map(|(&ref source, _)| source).collect();

        let exits_not_in_graph = !exits.iter()
            .any(|e| sinks.contains(e));
        let entry_not_in_graph = !sources.contains(&entry_node);

        if graph.is_empty() || exits_not_in_graph || entry_not_in_graph || exits.is_empty() {
            let kind = diesel::result::DatabaseErrorKind::CheckViolation;
            let msg = Box::new(ValidationErrorMessage{message: "Invalid username".to_string(),
                                                      column: "username".to_string(),
                                                      constraint_name: "username_limits".to_string()});
            return Err(diesel::result::Error::DatabaseError(kind, msg));
        }

        let flow = Self {
                id: Uuid::new_v4(),
                owner_id: author.id,
                created: chrono::Utc::now().naive_utc(),
                flow_name: flow_name.to_ascii_uppercase(),
                description,
                entry_node_id: entry_node.id
        };

        diesel::insert_into(crate::schema::flows::table)
            .values(&flow)
            .execute(conn)?;
        FlowConnection::connect_all(conn, flow.id, graph)?;
        FlowExit::create_exits(conn, flow.id, exits)?;
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

    pub fn list<C>(conn: &mut C,
                   page: u32,
                   page_size: u32) -> Vec<Self> 
        where C: Connection<Backend = Pg> + LoadConnection
    {
        let offset = page.saturating_sub(1) * page_size;
        match crate::schema::flows::table
                .limit(page_size as i64)
                .offset(offset as i64)
                .load::<Self>(conn) {
            Ok(list) => list,
            Err(err) => {
                tracing::warn!("DB List Query Failed: {:?}", err);
                vec![]
            }
        }
    }

    pub fn set_flow_entry<C>(&mut self, conn: &mut C, node: &FlowNode) -> QueryResult<()>
        where C: Connection<Backend = Pg> + LoadConnection
    {
        self.entry_node_id = node.id;
        FlowAssignment::assign(conn, self.id, node)?;
        Ok(())
    }
}

#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
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

    pub fn create<C>(conn: &mut C, node_name: &str) -> QueryResult<Self>
        where C: Connection<Backend = Pg>
    {
        let id = Uuid::new_v4();
        let new_flow_node = FlowNode{id, node_name: node_name.to_string()};
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
    pub fn edges<C>(conn: &mut C, node_id: Uuid) -> QueryResult<Vec<FlowNode>>
        where C: Connection<Backend = Pg> + LoadConnection
    {
        use crate::schema::flow_node_connections;
        use crate::schema::flow_nodes;

        let cnx = flow_node_connections::table
            .filter(flow_node_connections::dsl::from_node_id.eq(node_id))
            .load::<Self>(conn)?;
        let mut result = vec![];
        for c in cnx {
            let flow_node = flow_nodes::dsl::flow_nodes.find(c.to_node_id)
                .get_result::<FlowNode>(conn)?;
            result.push(flow_node);
        }
        Ok(result)
    }

    pub fn connect_all<C>(conn: &mut C, flow_id: Uuid, nodes: Vec<(&FlowNode, &FlowNode)>) -> QueryResult<()>
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
            FlowAssignment::assign(conn, flow_id, from_node)?;
            FlowAssignment::assign(conn, flow_id, to_node)?;
        }

        Ok(())
    }
}

#[derive(Queryable, Insertable, Clone, Debug)]
#[diesel(table_name = crate::schema::flow_assignments)]
pub struct FlowAssignment {
    pub flow_id: Uuid,
    pub node_id: Uuid,
}

impl FlowAssignment {
    pub fn assign<C>(conn: &mut C, flow_id: Uuid, flow_node: &FlowNode) -> QueryResult<()>
        where C: Connection<Backend = Pg>
    {
        let assignment = Self { flow_id, node_id: flow_node.id };
        match diesel::insert_into(crate::schema::flow_assignments::table)
            .values(&assignment)
            .execute(conn) {
            Err(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                _
            )) => {
                Ok(0usize)  // Unique errors mean already inserted, this is fine.
            }
            other => other,
        }?;
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
    pub fn create_exits<C>(conn: &mut C, flow_id: Uuid, exit_nodes: Vec<&FlowNode>) -> QueryResult<()>
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
            FlowAssignment::assign(conn, flow_id, &node)?;
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
        use crate::schema::flows;
        // Fetch entry point
        let flow = flows::table.find(flow_id)
            .get_result::<Flow>(conn)?;

        let entry_point = crate::schema::flow_nodes::table
            .find(flow.entry_node_id)
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
    use subseq_util::tables::harness::{to_pg_db_name, DbHarness};
    use function_name::named;

    #[test]
    #[named]
    fn test_flow_handle() {
        let db_name = to_pg_db_name(function_name!());
        let harness = DbHarness::new("localhost", "development", &db_name,
                                     Some(crate::tables::test::MIGRATIONS));
        let mut conn = harness.conn(); 

        let entry_node = FlowNode::create(&mut conn, "Open").expect("entry");
        let exit_node = FlowNode::create(&mut conn, "Closed").expect("entry");
        let graph = vec![(&entry_node, &exit_node)];
        let exits = vec![&exit_node];

        let user = User::create(&mut conn, Uuid::new_v4(), "test@example.com", None).expect("user");
        let flow = Flow::create(&mut conn,
                                &user,
                                "flow".to_string(),
                                "".to_string(),
                                &entry_node,
                                graph,
                                exits).expect("flow");
        let flow2 = Flow::get(&mut conn, flow.id).expect("task2");

        assert_eq!(flow, flow2);
        assert_eq!(flow.flow_name, "FLOW"); // Forced uppercase
    }
}
