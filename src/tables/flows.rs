use std::sync::Arc;
use diesel::prelude::*;
use tokio::sync::broadcast;
use uuid::Uuid;


#[derive(Queryable, Insertable, Clone, Debug)]
#[diesel(table_name = crate::schema::flows)]
pub struct Flow {
    id: Uuid,
    flow_name: String,
    description: String
}

#[derive(Queryable, Insertable, Clone, Debug)]
#[diesel(table_name = crate::schema::flow_nodes)]
pub struct FlowNode {
    id: Uuid,
    node_name: String
}

#[derive(Queryable, Insertable, Clone, Debug)]
#[diesel(table_name = crate::schema::flow_node_connections)]
pub struct FlowConnection {
    from_node_id: Uuid,
    to_node_id: Uuid 
}

#[derive(Queryable, Insertable, Clone, Debug)]
#[diesel(table_name = crate::schema::flow_assignments)]
pub struct FlowAssignment {
    flow_id: Uuid,
    node_id: Uuid,
}

#[derive(Queryable, Insertable, Clone, Debug)]
#[diesel(table_name = crate::schema::flow_entries)]
pub struct FlowEntry {
    flow_id: Uuid,
    node_id: Uuid,
}

#[derive(Queryable, Insertable, Clone, Debug)]
#[diesel(table_name = crate::schema::flow_exits)]
pub struct FlowExit {
    flow_id: Uuid,
    node_id: Uuid,
}
