CREATE TABLE sqlite_type_samples (
    id integer primary key autoincrement,
    enabled boolean default false not null,
    count_value integer default 0 not null,
    amount real default 0.0 not null,
    exact_amount numeric default 0,
    title text not null,
    payload json default null,
    created_on date default null,
    created_at datetime default current_timestamp,
    blob_value blob
);

CREATE TABLE sqlite_secondary_samples (
    id integer primary key,
    sample_id integer not null,
    status text default 'pending' not null
);
