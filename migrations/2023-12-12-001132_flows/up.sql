CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Create Flows Table
CREATE TABLE flows (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    flow_name VARCHAR NOT NULL,
    description TEXT NOT NULL
);

-- Create Flow Nodes Table
CREATE TABLE flow_nodes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    node_name VARCHAR NOT NULL
);

-- Create Flow Assignments for Flow Nodes
CREATE TABLE flow_assignments (
    flow_id UUID NOT NULL REFERENCES flows(id),
    node_id UUID NOT NULL REFERENCES flow_nodes(id),
    PRIMARY KEY (flow_id, node_id)
);

-- Create Entry for Flow
CREATE TABLE flow_entries (
    flow_id UUID PRIMARY KEY REFERENCES flows(id),
    node_id UUID NOT NULL REFERENCES flow_nodes(id)
);

-- Create Exits for Flow
CREATE TABLE flow_exits (
    flow_id UUID NOT NULL REFERENCES flows(id),
    node_id UUID NOT NULL REFERENCES flow_nodes(id),
    PRIMARY KEY (flow_id, node_id)
);

-- Create Relationship Table for Flow Nodes (Many-to-Many)
CREATE TABLE flow_node_connections (
    from_node_id UUID NOT NULL REFERENCES flow_nodes(id),
    to_node_id UUID NOT NULL REFERENCES flow_nodes(id),
    PRIMARY KEY (from_node_id, to_node_id)
);

