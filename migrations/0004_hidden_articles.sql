-- Articles a rule keeps out stay stored but hidden, so changing the rules can bring them back.
ALTER TABLE items ADD COLUMN hidden_at INTEGER;

-- Hidden articles are real rows now; this only remembered which ones had been left out.
DROP TABLE filtered_items;
