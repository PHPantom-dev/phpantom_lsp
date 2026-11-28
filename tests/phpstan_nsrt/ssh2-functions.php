<?php

use function PHPStan\Testing\assertType;

/** @var resource $resource */
$resource = doFoo();
$ssh2SftpStat = ssh2_sftp_stat($resource, __FILE__);

assertType('array{0: int, 1: int, 2: int, 3: int, 4: int, 5: int, 6: int, 7: int, 8: int, 9: int, 10: int, 11: int, 12: int, dev: int, ino: int, mode: int, nlink: int, uid: int, gid: int, rdev: int, size: int, atime: int, mtime: int, ctime: int, blksize: int, blocks: int}|false', $ssh2SftpStat);
