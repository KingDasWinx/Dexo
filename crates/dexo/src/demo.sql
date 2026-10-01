-- `dexo --demo`: a small shop to try every screen on. Every row is computed from its
-- number, so the store is the same on every machine and every run.

PRAGMA foreign_keys = ON;

CREATE TABLE customers (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL UNIQUE,
    city TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE products (
    id INTEGER PRIMARY KEY,
    sku TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    price NUMERIC NOT NULL CHECK (price > 0),
    stock INTEGER NOT NULL CHECK (stock >= 0)
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    customer_id INTEGER NOT NULL REFERENCES customers (id),
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid', 'shipped', 'delivered', 'cancelled')),
    ordered_at TEXT NOT NULL
);

CREATE TABLE order_items (
    order_id INTEGER NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
    product_id INTEGER NOT NULL REFERENCES products (id),
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    unit_price NUMERIC NOT NULL,
    PRIMARY KEY (order_id, product_id)
);

CREATE INDEX orders_by_customer ON orders (customer_id);
CREATE INDEX order_items_by_product ON order_items (product_id);

WITH RECURSIVE n (i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 120)
INSERT INTO customers (id, name, email, city, created_at)
SELECT
    i,
    json_extract('["Ana","Bruno","Carla","Diego","Elisa","Felipe","Gabriela","Hugo","Isabel","João","Karina","Lucas"]', '$[' || (i % 12) || ']')
        || ' '
        || json_extract('["Silva","Souza","Costa","Lima","Pereira","Alves","Rocha","Martins","Barros","Teixeira"]', '$[' || (i * 7 % 10) || ']'),
    'customer' || i || '@example.com',
    json_extract('["São Paulo","Lisboa","Porto Alegre","Recife","Curitiba","Belo Horizonte","Florianópolis","Salvador"]', '$[' || (i * 3 % 8) || ']'),
    date('2025-01-01', '+' || (i * 3) || ' days')
FROM n;

WITH RECURSIVE n (i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 40)
INSERT INTO products (id, sku, name, category, price, stock)
SELECT
    i,
    printf('SKU-%04d', i),
    json_extract('["Notebook","Mouse","Keyboard","Monitor","Headset","Webcam","Dock","Cable","Chair","Desk"]', '$[' || (i % 10) || ']')
        || ' '
        || json_extract('["Lite","Pro","Max","Mini"]', '$[' || (i % 4) || ']'),
    json_extract('["computers","peripherals","peripherals","displays","audio","video","accessories","accessories","furniture","furniture"]', '$[' || (i % 10) || ']'),
    round(9.9 + (i * 37 % 200) * 4.75, 2),
    i * 13 % 90
FROM n;

WITH RECURSIVE n (i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 300)
INSERT INTO orders (id, customer_id, status, ordered_at)
SELECT
    i,
    -- Ten customers never order, for the outer joins.
    i * 37 % 110 + 1,
    json_extract('["delivered","delivered","shipped","paid","pending","delivered","cancelled"]', '$[' || (i % 7) || ']'),
    datetime('2026-01-01 08:00:00', '+' || (i * 19) || ' hours')
FROM n;

-- One to four lines an order; 13 steps apart, so an order never lists a product twice.
WITH RECURSIVE n (i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 300),
lines (j) AS (SELECT 1 UNION ALL SELECT j + 1 FROM lines WHERE j < 4)
INSERT INTO order_items (order_id, product_id, quantity, unit_price)
SELECT n.i, p.id, (n.i + lines.j) % 3 + 1, p.price
FROM n
JOIN lines ON lines.j <= n.i % 4 + 1
JOIN products p ON p.id = (n.i * 7 + n.i / 40 + lines.j * 13) % 40 + 1;

CREATE VIEW order_totals AS
SELECT
    o.id AS order_id,
    c.name AS customer,
    o.status,
    o.ordered_at,
    round(sum(i.quantity * i.unit_price), 2) AS total
FROM orders o
JOIN customers c ON c.id = o.customer_id
JOIN order_items i ON i.order_id = o.id
GROUP BY o.id;
