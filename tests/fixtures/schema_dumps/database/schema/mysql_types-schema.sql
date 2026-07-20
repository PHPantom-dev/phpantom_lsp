CREATE TABLE `mysql_type_samples` (
    `id` bigint unsigned NOT NULL AUTO_INCREMENT,
    `tiny_flag` tinyint(1) DEFAULT 0 NOT NULL,
    `small_number` smallint DEFAULT 1 NOT NULL,
    `normal_number` int DEFAULT 42 NOT NULL,
    `big_number` bigint NOT NULL,
    `exact_amount` decimal(12,4) DEFAULT 0.0000 NOT NULL,
    `approx_amount` double DEFAULT NULL,
    `title` varchar(255) NOT NULL,
    `body` text,
    `payload` json DEFAULT NULL,
    `birthday` date DEFAULT NULL,
    `starts_at` time DEFAULT NULL,
    `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP,
    `updated_at` datetime DEFAULT NULL,
    `blob_value` longblob,
    PRIMARY KEY (`id`)
);

CREATE TABLE `mysql_secondary_samples` (
    `id` int unsigned NOT NULL AUTO_INCREMENT,
    `sample_id` bigint unsigned NOT NULL,
    `status` varchar(50) DEFAULT 'pending' NOT NULL,
    PRIMARY KEY (`id`)
);
