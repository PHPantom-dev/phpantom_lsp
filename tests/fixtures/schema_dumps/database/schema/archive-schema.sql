CREATE TABLE users (
    id integer primary key autoincrement,
    email text not null,
    archived_at text default null
);
