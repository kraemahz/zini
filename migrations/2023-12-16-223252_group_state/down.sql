ALTER TABLE projects DROP COLUMN default_flow_id;

DELETE FROM flow_exits where flow_id='00000000-0000-0000-0000-000000000000'::uuid;
DELETE FROM flow_nodes where id='00000000-0000-0000-0000-000000000000'::uuid;
DELETE FROM flow_nodes where id='00000000-0000-0000-0000-000000000001'::uuid;
DELETE FROM flows where id='00000000-0000-0000-0000-000000000000'::uuid;
DELETE FROM users where id='00000000-0000-0000-0000-000000000000'::uuid;

CREATE TABLE components (
    name VARCHAR PRIMARY KEY
);

CREATE TABLE task_components (
    task_id UUID NOT NULL REFERENCES tasks(id),
    component_name VARCHAR NOT NULL REFERENCES components(name),
    PRIMARY KEY (task_id, component_name)
);

CREATE TABLE flow_entries (
    flow_id UUID PRIMARY KEY REFERENCES flows(id),
    node_id UUID NOT NULL REFERENCES flow_nodes(id)
);

ALTER TABLE flows DROP COLUMN entry_node_id;

DROP TABLE task_links;
DROP TABLE link_types;
DROP TABLE default_project_tags;
DROP TABLE task_flows;
