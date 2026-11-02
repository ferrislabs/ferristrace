DELETE FROM identity_links slashed
USING identity_links bare
WHERE slashed.issuer <> rtrim(slashed.issuer, '/')
  AND bare.issuer = rtrim(slashed.issuer, '/')
  AND bare.subject = slashed.subject;

UPDATE identity_links
SET issuer = rtrim(issuer, '/')
WHERE issuer <> rtrim(issuer, '/');
