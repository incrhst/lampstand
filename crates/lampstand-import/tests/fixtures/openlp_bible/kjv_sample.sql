-- Hand-made, in the shape of OpenLP's bible schema (openlp/plugins/bibles/lib/db.py).
-- Text: King James Version, public domain. The test builds the .sqlite from this.
CREATE TABLE metadata (key VARCHAR(255) PRIMARY KEY, value VARCHAR(255));
CREATE TABLE book (id INTEGER PRIMARY KEY, book_reference_id INTEGER, testament_reference_id INTEGER, name VARCHAR(50));
CREATE TABLE verse (id INTEGER PRIMARY KEY, book_id INTEGER, chapter INTEGER, verse INTEGER, text TEXT);
INSERT INTO metadata VALUES ('name', 'King James Version (sample)');
-- book.id is not the canonical number: Psalms comes first here.
INSERT INTO book VALUES (1, 19, 1, 'Psalms');
INSERT INTO book VALUES (2, 1, 1, 'Genesis');
INSERT INTO book VALUES (3, 62, 2, '1 John');
-- reference id out of range: resolved by name
INSERT INTO book VALUES (4, 0, 2, 'John');
INSERT INTO verse VALUES (1, 1, 23, 1, 'The LORD is my shepherd; I shall not want.');
INSERT INTO verse VALUES (2, 1, 23, 2, 'He maketh me to lie down in green pastures: he leadeth me beside the still waters.');
INSERT INTO verse VALUES (3, 2, 1, 1, 'In the beginning God created the heaven and the earth.');
INSERT INTO verse VALUES (4, 2, 1, 2, 'And the earth was without form, and void; and darkness was upon the face of the deep. And the Spirit of God moved upon the face of the waters.');
INSERT INTO verse VALUES (5, 2, 1, 3, 'And God said, Let there be light: and there was light.');
INSERT INTO verse VALUES (6, 3, 1, 1, 'That which was from the beginning, which we have heard, which we have seen with our eyes, which we have looked upon, and our hands have handled, of the Word of life;');
INSERT INTO verse VALUES (7, 4, 3, 16, 'For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.');
