-- Milestone 5.A: completion reads the Commands that start with a prefix as
-- one index range (text >= prefix AND text < prefix || U+10FFFF).
CREATE INDEX commands_text ON commands(text);
