DROP TABLE task_components;
DROP TABLE components;

-- Attach flows to every project so a task is created with it
INSERT INTO users (id, email, created, salt) VALUES (
    '00000000-0000-0000-0000-000000000000'::uuid,
    'support@subseq.io',
    NOW(),
    E'\\x6b3333703332407375627365712e696f'
);
INSERT INTO flows (id, owner_id, created, flow_name, description) VALUES (
    '00000000-0000-0000-0000-000000000000'::uuid,
    '00000000-0000-0000-0000-000000000000'::uuid,
    NOW(),
    'Default',
    'This is the default flow'
);

INSERT INTO flow_nodes (id, node_name) VALUES (
    '00000000-0000-0000-0000-000000000000'::uuid,
    'OPEN'
), (
    '00000000-0000-0000-0000-000000000001'::uuid,
    'CLOSED'
);
INSERT INTO flow_assignments (flow_id, node_id) VALUES (
    '00000000-0000-0000-0000-000000000000'::uuid,
    '00000000-0000-0000-0000-000000000000'::uuid
), (
    '00000000-0000-0000-0000-000000000000'::uuid,
    '00000000-0000-0000-0000-000000000001'::uuid
);
INSERT INTO flow_node_connections (from_node_id, to_node_id) VALUES (
    '00000000-0000-0000-0000-000000000000'::uuid,
    '00000000-0000-0000-0000-000000000001'::uuid
);
INSERT INTO flow_exits (flow_id, node_id) VALUES (
    '00000000-0000-0000-0000-000000000000'::uuid,
    '00000000-0000-0000-0000-000000000001'::uuid
);

ALTER TABLE projects ADD COLUMN default_flow_id UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000000'::uuid;

-- Always store a node entry on the flow
DROP TABLE flow_entries;
ALTER TABLE flows ADD COLUMN entry_node_id UUID NOT NULL
    REFERENCES flow_nodes(id)
    DEFAULT '00000000-0000-0000-0000-000000000000'::uuid;

-- Create default tags for projects to add to tasks
CREATE TABLE default_project_tags (
    project_id UUID REFERENCES projects(id),
    tag_name VARCHAR REFERENCES tags(name),
    PRIMARY KEY (project_id, tag_name)
);

-- Create state entry for the current postion of a task in a flow
CREATE TABLE task_flows (
    task_id UUID REFERENCES tasks(id),
    flow_id UUID REFERENCES flows(id),
    current_node_id UUID REFERENCES flow_nodes(id),
    order_added INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (task_id, flow_id)
);

-- Links allow us to specify information about relationships between tasks
CREATE TABLE link_types (
    id SERIAL PRIMARY KEY,
    link_name VARCHAR UNIQUE NOT NULL
);

INSERT INTO link_types (link_name) VALUES ('SUBTASK OF'), ('DEPENDS ON'), ('RELATED TO');

CREATE TABLE task_links (
    task_from_id UUID REFERENCES tasks(id),
    task_to_id UUID REFERENCES tasks(id),
    link_type INTEGER NOT NULL REFERENCES link_types(id),
    PRIMARY KEY (task_from_id, task_to_id)
)
