-- An account carried an address and nothing else, so every screen that wanted to name somebody
-- showed the address: the top bar, the members table, the profile card. The column is nullable
-- because an account exists before anybody writes a name into it -- an invitation creates one
-- from an address alone -- and because somebody who wrote a name must be able to take it back.
-- Absent means the address is the name, which is what the screens did already.
ALTER TABLE accounts ADD COLUMN name text;

-- A blank name is not a name: it would print as an empty line where the address used to be, and
-- the two states would be indistinguishable to a reader. The check says in the schema what
-- AccountName says in core, so a hand-written UPDATE cannot create a row the code calls absent.
ALTER TABLE accounts ADD CONSTRAINT accounts_name_is_not_blank
    CHECK (name IS NULL OR btrim(name) <> '');
