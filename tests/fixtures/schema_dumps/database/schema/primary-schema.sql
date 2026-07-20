CREATE TABLE public.users (
    id bigserial PRIMARY KEY,
    email character varying(255) NOT NULL,
    display_name text DEFAULT 'Guest',
    settings jsonb DEFAULT '{}'::jsonb,
    active boolean DEFAULT true NOT NULL,
    created_at timestamp without time zone,
    CONSTRAINT users_email_unique UNIQUE (email)
);

CREATE TABLE public.orders (
    id bigint NOT NULL,
    user_id bigint NOT NULL,
    total numeric(10, 2) DEFAULT 0 NOT NULL,
    metadata json,
    PRIMARY KEY (id)
);
