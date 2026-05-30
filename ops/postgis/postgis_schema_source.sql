-- Source PostGIS database: regions with polygon geometry
CREATE TABLE IF NOT EXISTS regions (
    id       SERIAL PRIMARY KEY,
    name     VARCHAR(100) NOT NULL,
    shape    geometry(Polygon, 4326)
);

INSERT INTO regions (name, shape) VALUES
    ('Square',
     ST_GeomFromText('POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))', 4326)),

    ('Triangle',
     ST_GeomFromText('POLYGON((0 0, 10 0, 5 8, 0 0))', 4326)),

    ('Rectangle',
     ST_GeomFromText('POLYGON((0 0, 20 0, 20 5, 0 5, 0 0))', 4326)),

    ('Diamond',
     ST_GeomFromText('POLYGON((5 0, 10 5, 5 10, 0 5, 5 0))', 4326)),

    ('Pentagon',
     ST_GeomFromText('POLYGON((5 0, 10 3, 8 9, 2 9, 0 3, 5 0))', 4326));
