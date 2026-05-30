-- Destination MySQL database: computed geometry metrics
CREATE DATABASE IF NOT EXISTS regions_dest;
USE regions_dest;

CREATE TABLE IF NOT EXISTS region_areas (
    id   INT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    area DOUBLE       NOT NULL
);

CREATE TABLE IF NOT EXISTS region_perimeters (
    id        INT AUTO_INCREMENT PRIMARY KEY,
    name      VARCHAR(100) NOT NULL,
    perimeter DOUBLE       NOT NULL
);
