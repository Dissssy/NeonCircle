-- Add migration script here

-- alter the users table and change the default timezone to 'EST5EDT'   
ALTER TABLE users
    ALTER COLUMN timezone SET DEFAULT 'EST5EDT';