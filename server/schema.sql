
SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public;

COMMENT ON EXTENSION vector IS 'vector data type and ivfflat and hnsw access methods';

CREATE FUNCTION public.nearest_faces(query public.vector, k integer DEFAULT 50) RETURNS TABLE(user_id uuid, distance double precision)
    LANGUAGE sql STABLE
    SET "hnsw.iterative_scan" TO 'relaxed_order'
    SET "hnsw.ef_search" TO '100'
    SET enable_seqscan TO 'off'
    AS $$
    WITH nearest AS MATERIALIZED (
        SELECT f.user_id, f.embedding <=> query AS distance
        FROM face_embeddings f
        ORDER BY f.embedding <=> query
        LIMIT k
    )
    SELECT n.user_id, n.distance FROM nearest n WHERE n.distance <= 0.32 ORDER BY n.distance
$$;

CREATE FUNCTION public.set_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at := now();
    RETURN NEW;
END
$$;

SET default_tablespace = '';

SET default_table_access_method = heap;

CREATE TABLE public.entitlement_scans (
    user_id uuid NOT NULL,
    entitlement_id character varying(32) NOT NULL,
    scanned_by uuid,
    client_scanned_at timestamp with time zone NOT NULL,
    server_synced_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.entitlements (
    entitlement_id character varying(32) NOT NULL,
    label text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT entitlements_entitlement_id_check CHECK (((entitlement_id)::text ~ '^[a-z0-9-]+$'::text))
);

CREATE TABLE public.face_embeddings (
    user_id uuid NOT NULL,
    embedding public.vector(512) NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.hardware_assets (
    asset_qr character varying(64) NOT NULL,
    category text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.hardware_loans (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    asset_qr character varying(64) NOT NULL,
    user_id uuid NOT NULL,
    checked_out_by uuid,
    checked_out_at timestamp with time zone DEFAULT now() NOT NULL,
    returned_at timestamp with time zone,
    CONSTRAINT hardware_loans_return_after_checkout CHECK ((returned_at >= checked_out_at))
);

CREATE TABLE public.judge_exchange_audit (
    id bigint NOT NULL,
    room_id character varying(32),
    at timestamp with time zone NOT NULL,
    user_agent text,
    ip text,
    outcome text NOT NULL,
    session_id uuid,
    CONSTRAINT judge_exchange_audit_check CHECK (((outcome = 'ok'::text) OR (session_id IS NULL))),
    CONSTRAINT judge_exchange_audit_outcome_check CHECK ((outcome = ANY (ARRAY['ok'::text, 'malformed'::text, 'bad_signature'::text, 'unknown_room'::text, 'old_version'::text, 'outside_window'::text])))
);

ALTER TABLE public.judge_exchange_audit ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.judge_exchange_audit_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);

CREATE TABLE public.judging_queue (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    room_id character varying(32) NOT NULL,
    team_id uuid NOT NULL,
    track character varying(48) NOT NULL,
    seq_index integer NOT NULL,
    status character varying(20) DEFAULT 'QUEUED'::character varying NOT NULL,
    raw_score double precision,
    prev_team_id uuid,
    beat_prev_team boolean,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT judging_queue_seq_index_check CHECK ((seq_index >= 0)),
    CONSTRAINT judging_queue_status_check CHECK (((status)::text = ANY ((ARRAY['QUEUED'::character varying, 'IN_THE_HOLE'::character varying, 'ON_DECK'::character varying, 'PRESENTING'::character varying, 'DONE'::character varying, 'SKIPPED'::character varying])::text[])))
);

CREATE TABLE public.mentor_tickets (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    team_id uuid,
    location_label character varying(64) NOT NULL,
    svg_x real NOT NULL,
    svg_y real NOT NULL,
    tags text[] NOT NULL,
    description text NOT NULL,
    status character varying(24) DEFAULT 'OPEN'::character varying NOT NULL,
    claimed_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    claimed_at timestamp with time zone,
    resolved_at timestamp with time zone,
    CONSTRAINT mentor_tickets_status_check CHECK (((status)::text = ANY ((ARRAY['OPEN'::character varying, 'CLAIMED'::character varying, 'RESOLVED'::character varying, 'CANCELED'::character varying])::text[])))
);

CREATE TABLE public.notification_dispatches (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    event_type character varying(48) NOT NULL,
    priority smallint NOT NULL,
    push_sent_at timestamp with time zone,
    push_acked_at timestamp with time zone,
    sms_triggered_at timestamp with time zone,
    sms_status character varying(24),
    email_triggered_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT notification_dispatches_priority_check CHECK ((priority >= 1)),
    CONSTRAINT notification_dispatches_sms_status_check CHECK (((sms_status)::text = ANY ((ARRAY['DELIVERED'::character varying, 'FAILED'::character varying])::text[])))
);

CREATE TABLE public.presentation_rooms (
    room_id character varying(32) NOT NULL,
    track character varying(48) NOT NULL,
    floor_number smallint NOT NULL,
    svg_x real NOT NULL,
    svg_y real NOT NULL,
    token_version integer DEFAULT 1 NOT NULL,
    token_hash bytea,
    judging_starts_at timestamp with time zone,
    judging_ends_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT presentation_rooms_room_id_check CHECK (((room_id)::text ~ '^[A-Za-z0-9_-]+$'::text)),
    CONSTRAINT presentation_rooms_token_version_check CHECK ((token_version > 0)),
    CONSTRAINT presentation_rooms_window CHECK ((judging_ends_at > judging_starts_at))
);

CREATE TABLE public.sessions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    revoked_at timestamp with time zone,
    room_id character varying(32),
    CONSTRAINT sessions_check CHECK ((expires_at > created_at)),
    CONSTRAINT sessions_user_or_room CHECK (((user_id IS NULL) <> (room_id IS NULL)))
);

CREATE TABLE public.sponsor_keys (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    sponsor_slug character varying(48) NOT NULL,
    key_value text NOT NULL,
    claimed_by_team uuid,
    claimed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT sponsor_keys_claim_complete CHECK (((claimed_by_team IS NULL) = (claimed_at IS NULL)))
);

CREATE TABLE public.team_members (
    user_id uuid NOT NULL,
    team_id uuid NOT NULL,
    joined_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.teams (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    hacking_zone_id character varying(32),
    svg_x real,
    svg_y real,
    devpost_url text,
    opted_tracks text[] DEFAULT '{}'::text[] NOT NULL,
    heartbeat_confirmed_at timestamp with time zone,
    bt_theta double precision DEFAULT 0.0 NOT NULL,
    final_composite_score double precision,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.user_roles (
    user_id uuid NOT NULL,
    role text NOT NULL,
    CONSTRAINT user_roles_role_check CHECK ((role = ANY (ARRAY['HACKER'::text, 'VOLUNTEER'::text, 'MENTOR'::text, 'ADMIN'::text])))
);

CREATE TABLE public.users (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    email text,
    phone_e164 text,
    full_name text,
    school text,
    dietary_flags smallint DEFAULT 0 NOT NULL,
    qr_hmac_sig bytea,
    checked_in_at timestamp with time zone,
    collateral_held boolean DEFAULT false NOT NULL,
    travel_reimbursement_cap_cents integer DEFAULT 0 NOT NULL,
    apns_fcm_tokens text[] DEFAULT '{}'::text[] NOT NULL,
    webpush_sub jsonb,
    live_activity_token text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT users_email_check CHECK ((email = lower(email))),
    CONSTRAINT users_email_or_phone CHECK (((email IS NOT NULL) OR (phone_e164 IS NOT NULL))),
    CONSTRAINT users_phone_e164_check CHECK ((phone_e164 ~ '^\+[1-9][0-9]{1,14}$'::text)),
    CONSTRAINT users_travel_reimbursement_cap_cents_check CHECK ((travel_reimbursement_cap_cents >= 0))
);

ALTER TABLE ONLY public.entitlement_scans
    ADD CONSTRAINT entitlement_scans_pkey PRIMARY KEY (user_id, entitlement_id);

ALTER TABLE ONLY public.entitlements
    ADD CONSTRAINT entitlements_pkey PRIMARY KEY (entitlement_id);

ALTER TABLE ONLY public.face_embeddings
    ADD CONSTRAINT face_embeddings_pkey PRIMARY KEY (user_id);

ALTER TABLE ONLY public.hardware_assets
    ADD CONSTRAINT hardware_assets_pkey PRIMARY KEY (asset_qr);

ALTER TABLE ONLY public.hardware_loans
    ADD CONSTRAINT hardware_loans_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.judge_exchange_audit
    ADD CONSTRAINT judge_exchange_audit_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.judging_queue
    ADD CONSTRAINT judging_queue_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.judging_queue
    ADD CONSTRAINT judging_queue_room_seq UNIQUE (room_id, seq_index) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE ONLY public.mentor_tickets
    ADD CONSTRAINT mentor_tickets_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.notification_dispatches
    ADD CONSTRAINT notification_dispatches_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.presentation_rooms
    ADD CONSTRAINT presentation_rooms_pkey PRIMARY KEY (room_id);

ALTER TABLE ONLY public.presentation_rooms
    ADD CONSTRAINT presentation_rooms_token_hash_key UNIQUE (token_hash);

ALTER TABLE ONLY public.sessions
    ADD CONSTRAINT sessions_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.sponsor_keys
    ADD CONSTRAINT sponsor_keys_key_value_key UNIQUE (key_value);

ALTER TABLE ONLY public.sponsor_keys
    ADD CONSTRAINT sponsor_keys_one_per_team UNIQUE (claimed_by_team, sponsor_slug);

ALTER TABLE ONLY public.sponsor_keys
    ADD CONSTRAINT sponsor_keys_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.team_members
    ADD CONSTRAINT team_members_pkey PRIMARY KEY (user_id);

ALTER TABLE ONLY public.teams
    ADD CONSTRAINT teams_name_key UNIQUE (name);

ALTER TABLE ONLY public.teams
    ADD CONSTRAINT teams_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.user_roles
    ADD CONSTRAINT user_roles_pkey PRIMARY KEY (user_id, role);

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_email_key UNIQUE (email);

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_phone_e164_key UNIQUE (phone_e164);

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_qr_hmac_sig_key UNIQUE (qr_hmac_sig);

CREATE INDEX entitlement_scans_entitlement_id ON public.entitlement_scans USING btree (entitlement_id);

CREATE INDEX entitlement_scans_scanned_by ON public.entitlement_scans USING btree (scanned_by);

CREATE INDEX face_embeddings_embedding ON public.face_embeddings USING hnsw (embedding public.vector_cosine_ops) WITH (m='16', ef_construction='64');

CREATE INDEX hardware_loans_asset_qr ON public.hardware_loans USING btree (asset_qr, checked_out_at);

CREATE INDEX hardware_loans_checked_out_by ON public.hardware_loans USING btree (checked_out_by);

CREATE UNIQUE INDEX hardware_loans_one_open_per_asset ON public.hardware_loans USING btree (asset_qr) WHERE (returned_at IS NULL);

CREATE INDEX hardware_loans_user_id ON public.hardware_loans USING btree (user_id);

CREATE INDEX judge_exchange_audit_room_at ON public.judge_exchange_audit USING btree (room_id, at);

CREATE INDEX judge_exchange_audit_session_id ON public.judge_exchange_audit USING btree (session_id);

CREATE INDEX judging_queue_prev_team_id ON public.judging_queue USING btree (prev_team_id);

CREATE INDEX judging_queue_team_id ON public.judging_queue USING btree (team_id);

CREATE INDEX mentor_tickets_claimed_by ON public.mentor_tickets USING btree (claimed_by);

CREATE INDEX mentor_tickets_open ON public.mentor_tickets USING btree (created_at) WHERE ((status)::text = 'OPEN'::text);

CREATE INDEX mentor_tickets_tags ON public.mentor_tickets USING gin (tags);

CREATE INDEX mentor_tickets_team_id ON public.mentor_tickets USING btree (team_id);

CREATE INDEX notification_dispatches_unacked_critical ON public.notification_dispatches USING btree (push_sent_at) WHERE ((priority = 1) AND (push_acked_at IS NULL) AND (sms_triggered_at IS NULL));

CREATE INDEX notification_dispatches_user_id ON public.notification_dispatches USING btree (user_id);

CREATE INDEX sessions_room_id ON public.sessions USING btree (room_id) WHERE (room_id IS NOT NULL);

CREATE INDEX sessions_user_id ON public.sessions USING btree (user_id);

CREATE INDEX sponsor_keys_unclaimed ON public.sponsor_keys USING btree (sponsor_slug, id) WHERE (claimed_by_team IS NULL);

CREATE INDEX team_members_team_id ON public.team_members USING btree (team_id);

CREATE TRIGGER entitlements_updated_at BEFORE UPDATE ON public.entitlements FOR EACH ROW EXECUTE FUNCTION public.set_updated_at();

CREATE TRIGGER face_embeddings_updated_at BEFORE UPDATE ON public.face_embeddings FOR EACH ROW EXECUTE FUNCTION public.set_updated_at();

CREATE TRIGGER hardware_assets_updated_at BEFORE UPDATE ON public.hardware_assets FOR EACH ROW EXECUTE FUNCTION public.set_updated_at();

CREATE TRIGGER judging_queue_updated_at BEFORE UPDATE ON public.judging_queue FOR EACH ROW EXECUTE FUNCTION public.set_updated_at();

CREATE TRIGGER mentor_tickets_updated_at BEFORE UPDATE ON public.mentor_tickets FOR EACH ROW EXECUTE FUNCTION public.set_updated_at();

CREATE TRIGGER notification_dispatches_updated_at BEFORE UPDATE ON public.notification_dispatches FOR EACH ROW EXECUTE FUNCTION public.set_updated_at();

CREATE TRIGGER presentation_rooms_updated_at BEFORE UPDATE ON public.presentation_rooms FOR EACH ROW EXECUTE FUNCTION public.set_updated_at();

CREATE TRIGGER teams_updated_at BEFORE UPDATE ON public.teams FOR EACH ROW EXECUTE FUNCTION public.set_updated_at();

CREATE TRIGGER users_updated_at BEFORE UPDATE ON public.users FOR EACH ROW EXECUTE FUNCTION public.set_updated_at();

ALTER TABLE ONLY public.entitlement_scans
    ADD CONSTRAINT entitlement_scans_entitlement_id_fkey FOREIGN KEY (entitlement_id) REFERENCES public.entitlements(entitlement_id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.entitlement_scans
    ADD CONSTRAINT entitlement_scans_scanned_by_fkey FOREIGN KEY (scanned_by) REFERENCES public.users(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.entitlement_scans
    ADD CONSTRAINT entitlement_scans_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.face_embeddings
    ADD CONSTRAINT face_embeddings_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.hardware_loans
    ADD CONSTRAINT hardware_loans_asset_qr_fkey FOREIGN KEY (asset_qr) REFERENCES public.hardware_assets(asset_qr) ON DELETE RESTRICT;

ALTER TABLE ONLY public.hardware_loans
    ADD CONSTRAINT hardware_loans_checked_out_by_fkey FOREIGN KEY (checked_out_by) REFERENCES public.users(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.hardware_loans
    ADD CONSTRAINT hardware_loans_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.judge_exchange_audit
    ADD CONSTRAINT judge_exchange_audit_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.sessions(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.judging_queue
    ADD CONSTRAINT judging_queue_prev_team_id_fkey FOREIGN KEY (prev_team_id) REFERENCES public.teams(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.judging_queue
    ADD CONSTRAINT judging_queue_room_id_fkey FOREIGN KEY (room_id) REFERENCES public.presentation_rooms(room_id) ON DELETE CASCADE;

ALTER TABLE ONLY public.judging_queue
    ADD CONSTRAINT judging_queue_team_id_fkey FOREIGN KEY (team_id) REFERENCES public.teams(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.mentor_tickets
    ADD CONSTRAINT mentor_tickets_claimed_by_fkey FOREIGN KEY (claimed_by) REFERENCES public.users(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.mentor_tickets
    ADD CONSTRAINT mentor_tickets_team_id_fkey FOREIGN KEY (team_id) REFERENCES public.teams(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.notification_dispatches
    ADD CONSTRAINT notification_dispatches_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.sessions
    ADD CONSTRAINT sessions_room_id_fkey FOREIGN KEY (room_id) REFERENCES public.presentation_rooms(room_id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.sessions
    ADD CONSTRAINT sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.sponsor_keys
    ADD CONSTRAINT sponsor_keys_claimed_by_team_fkey FOREIGN KEY (claimed_by_team) REFERENCES public.teams(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.team_members
    ADD CONSTRAINT team_members_team_id_fkey FOREIGN KEY (team_id) REFERENCES public.teams(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.team_members
    ADD CONSTRAINT team_members_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.user_roles
    ADD CONSTRAINT user_roles_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

