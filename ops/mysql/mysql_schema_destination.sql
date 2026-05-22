create table if not exists spot
(
	id int auto_increment primary key,
	address varchar(255) null,
	number int null,
	name varchar(255) null,
	surname varchar(255) null
);

create table if not exists order_totals_plus_ten
(
	id int auto_increment primary key,
	amount int null
);

create table if not exists sensor_weights_scaled
(
	id int auto_increment primary key,
	weight int null
);

create table if not exists customer_aliases_unique
(
	id int auto_increment primary key,
	email varchar(255) null,
	label varchar(255) null
);
