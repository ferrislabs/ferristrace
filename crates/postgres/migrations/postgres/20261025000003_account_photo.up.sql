-- A photo lives in an object store, not here: the bytes are somebody's face and the control plane
-- is the wrong place for a megabyte a row. What is kept is where to find them -- a token the
-- store named the object after -- and what they are, so an answer can carry a content type
-- without asking the store first.
--
-- The token changes with every upload. That is the point: the url a screen reads carries it, so
-- the photo can be cached for ever and still change the moment somebody replaces it.
ALTER TABLE accounts ADD COLUMN photo_token text;
ALTER TABLE accounts ADD COLUMN photo_type text;

-- Half a photo is no photo: a token with no type cannot be served and a type with no token names
-- nothing. The two columns are one fact, so the schema says so.
ALTER TABLE accounts ADD CONSTRAINT accounts_photo_is_whole
    CHECK ((photo_token IS NULL) = (photo_type IS NULL));
