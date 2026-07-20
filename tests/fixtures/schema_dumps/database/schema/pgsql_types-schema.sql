--
-- PostgreSQL database dump
--

\restrict synthetic_token

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: lquery; Type: SHELL TYPE; Schema: public; Owner: -
--

CREATE TYPE public.lquery;

--
-- Name: lquery_in(cstring); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.lquery_in(cstring) RETURNS public.lquery
    LANGUAGE c STRICT
    AS '$libdir/ltree', 'lquery_in';

--
-- Name: pg_type_samples; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.pg_type_samples (
    id bigserial PRIMARY KEY,
    small_number smallint DEFAULT 1 NOT NULL,
    normal_number integer DEFAULT 42 NOT NULL,
    big_number bigint NOT NULL,
    exact_amount numeric(12, 4) DEFAULT 0.0000 NOT NULL,
    approximate_amount double precision,
    real_amount real,
    active boolean DEFAULT false NOT NULL,
    fixed_code character(12),
    variable_name character varying(255) NOT NULL,
    description text,
    payload json,
    settings jsonb DEFAULT '{}'::jsonb,
    created_on date,
    starts_at time without time zone,
    starts_at_tz time with time zone,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    published_at timestamp with time zone,
    duration interval,
    public_id uuid DEFAULT gen_random_uuid() NOT NULL,
    ip inet,
    network cidr,
    mac macaddr,
    blob bytea,
    tags text[],
    scores integer[],
    CONSTRAINT pg_type_samples_public_id_unique UNIQUE (public_id)
);

--
-- Name: pg_secondary_samples; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.pg_secondary_samples (
    id bigint NOT NULL,
    sample_id bigint NOT NULL,
    status character varying(50) DEFAULT 'pending' NOT NULL,
    processed_at timestamp(0) without time zone,
    measured_at timestamp(6) with time zone,
    PRIMARY KEY (id)
);

--
-- Data for Name: migrations; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.migrations (id, migration, batch) FROM stdin;
1	2024_01_01_000001_create_type_samples_table	1
2	2024_01_01_000002_create_secondary_samples_table	1
\.

--
-- Name: pg_type_samples pg_type_samples_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.pg_type_samples_id_seq', 1, false);

--
-- PostgreSQL database dump complete
--

\unrestrict synthetic_token
