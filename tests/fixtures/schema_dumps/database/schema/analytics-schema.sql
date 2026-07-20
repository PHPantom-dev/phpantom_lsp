CREATE TABLE `users` (
    `id` bigint unsigned NOT NULL AUTO_INCREMENT,
    `email` varchar(255) NOT NULL,
    `last_seen_at` datetime DEFAULT NULL,
    `score` decimal(8,2) DEFAULT 0.00 NOT NULL,
    PRIMARY KEY (`id`)
);

CREATE TABLE `events` (
    `id` bigint unsigned NOT NULL AUTO_INCREMENT,
    `payload` json DEFAULT NULL,
    `processed` tinyint(1) DEFAULT 0 NOT NULL,
    PRIMARY KEY (`id`)
);
