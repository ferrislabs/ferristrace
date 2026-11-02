-- A logo lives in the object store beside the faces, for the same reason: the bytes are not the
-- control plane's to hold, and what is kept here is only where to find them -- a token the store
-- named the object after -- and what they are, so an answer can carry a content type without
-- asking the store first.
--
-- The token changes with every upload, so the url a screen reads changes with it and the logo can
-- be cached for ever while still turning over the moment an administrator replaces it.
ALTER TABLE orgs ADD COLUMN logo_token text;
ALTER TABLE orgs ADD COLUMN logo_type text;

-- Half a logo is no logo: a token with no type cannot be served and a type with no token names
-- nothing. The two columns are one fact, so the schema says so.
ALTER TABLE orgs ADD CONSTRAINT orgs_logo_is_whole
    CHECK ((logo_token IS NULL) = (logo_type IS NULL));
