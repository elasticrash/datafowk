create table if not exists address
(
	id int auto_increment,
	address varchar(255) null,
	number int null,
	constraint address_id_uindex unique (id)
);

create table if not exists users
(
	id int auto_increment primary key,
	firstname varchar(255) null,
	lastname varchar(255) null,
	age int null,
	address_id int null,
	occured_at datetime null,
	constraint users_address_id_fk foreign key (address_id) references address (id)
);

create table if not exists devices 
(
	id int auto_increment,
	name varchar(255) null,
	user_id int null,
	constraint devices_id_uindex unique (id)
);

create table if not exists order_totals
(
	id int auto_increment primary key,
	amount int null
);

create table if not exists sensor_weights
(
	id int auto_increment primary key,
	weight int null
);

insert into address (id, address, number)
values
    (1, 'Baker Street', 221),
    (2, 'Fleet Street', 13)
on duplicate key update
    address = values(address),
    number = values(number);

insert into users (id, firstname, lastname, age, address_id, occured_at)
values
    (1, 'Alice ', 'Smith', 28, 1, '2026-01-01 12:00:00'),
    (2, 'Bob ', 'Brown', 31, 2, '2026-01-02 13:30:00')
on duplicate key update
    firstname = values(firstname),
    lastname = values(lastname),
    age = values(age),
    address_id = values(address_id),
    occured_at = values(occured_at);

insert into devices (id, name, user_id)
values
    (1, 'Phase8', 1),
    (2, 'Assembler', 1)
on duplicate key update
    name = values(name),
    user_id = values(user_id);

insert into order_totals (id, amount)
values
    (1, 25),
    (2, 40)
on duplicate key update
    amount = values(amount);

insert into sensor_weights (id, weight)
values
    (1, 2),
    (2, 3)
on duplicate key update
    weight = values(weight);
