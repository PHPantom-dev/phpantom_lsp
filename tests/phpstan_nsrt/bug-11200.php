<?php declare(strict_types=1);

namespace Bug11200;

use function PHPStan\Testing\assertType;

class Bug11200SplFileObjectTests
{
  public function fflush() : void
  {
    $file = new \SplFileObject('php://memory', 'r');
    assertType('bool', $file->eof());
    if ( $file->eof() )
    {
      return;
    }
    assertType('false', $file->eof());
    // call method that has side effects
    $file->fflush();
    // the value of eof may have changed
    assertType('bool', $file->eof()); // SKIP: a narrowed member read or call result survives a call that can change it
  }
  
  public function fgetc() : void
  {
    $file = new \SplFileObject('php://memory', 'r');
    assertType('bool', $file->eof());
    if ( $file->eof() )
    {
      return;
    }
    assertType('false', $file->eof());
    // call method that has side effects
    $file->fgetc();
    // the value of eof may have changed
    assertType('bool', $file->eof()); // SKIP: a narrowed member read or call result survives a call that can change it
  }

  public function fgetcsv() : void
  {
    $file = new \SplFileObject('php://memory', 'r');
    assertType('bool', $file->eof());
    if ( $file->eof() )
    {
      return;
    }
    assertType('false', $file->eof());
    // call method that has side effects
    $file->fgetcsv();
    // the value of eof may have changed
    assertType('bool', $file->eof()); // SKIP: a narrowed member read or call result survives a call that can change it
  }

  public function fgets() : void
  {
    $file = new \SplFileObject('php://memory', 'r');
    assertType('bool', $file->eof());
    if ( $file->eof() )
    {
      return;
    }
    assertType('false', $file->eof());
    // call method that has side effects
    $file->fgets();
    // the value of eof may have changed
    assertType('bool', $file->eof()); // SKIP: a narrowed member read or call result survives a call that can change it
  }

  public function fpassthru() : void
  {
    $file = new \SplFileObject('php://memory', 'r');
    assertType('bool', $file->eof());
    if ( $file->eof() )
    {
      return;
    }
    assertType('false', $file->eof());
    // call method that has side effects
    $file->fpassthru();
    // the value of eof may have changed
    assertType('bool', $file->eof()); // SKIP: a narrowed member read or call result survives a call that can change it
  }

  public function fputcsv() : void
  {
    // places file pointer at the start of the file
    $file = new \SplFileObject('php://memory', 'rw+');
    assertType('int|false', $file->ftell());
    if ($file->ftell() !== 0)
    {
      return;
    }
    assertType('0', $file->ftell());
    // This file is not empty.
    // call method that has side effects
    $file->fputcsv(['a']);
    // the value of ftell may have changed
  }

  public function fread() : void
  {
    $file = new \SplFileObject('php://memory', 'r');
    assertType('bool', $file->eof());
    if ( $file->eof() )
    {
      return;
    }
    assertType('false', $file->eof());
    // call method that has side effects
    $file->fread(1);
    // the value of eof may have changed
    assertType('bool', $file->eof()); // SKIP: a narrowed member read or call result survives a call that can change it
  }

  public function fscanf() : void
  {
    $file = new \SplFileObject('php://memory', 'r');
    assertType('bool', $file->eof());
    if ( $file->eof() )
    {
      return;
    }
    assertType('false', $file->eof());
    // call method that has side effects
    $file->fscanf('%f');
    // the value of eof may have changed
    assertType('bool', $file->eof()); // SKIP: a narrowed member read or call result survives a call that can change it
  }

  public function fseek() : void
  {
    // places file pointer at the start of the file
    $file = new \SplFileObject('php://memory', 'rw+');
    assertType('int|false', $file->ftell());
    if ($file->ftell() !== 0)
    {
      return;
    }
    assertType('0', $file->ftell());
    // This file is not empty.
    // call method that has side effects
    $file->fseek(1,\SEEK_SET);
    // the value of ftell may have changed
  }

  public function ftruncate() : void
  {
    $file = new \SplFileObject('php://memory', 'r');
    assertType('bool', $file->eof());
    if ( $file->eof() )
    {
      return;
    }
    assertType('false', $file->eof());
    // call method that has side effects
    $file->ftruncate(0);
    // the value of eof may have changed
    assertType('bool', $file->eof()); // SKIP: a narrowed member read or call result survives a call that can change it
  }

  public function fwrite() : void
  {
    // places file pointer at the start of the file
    $file = new \SplFileObject('php://memory', 'rw+');
    assertType('int|false', $file->ftell());
    if ($file->ftell() !== 0)
    {
      return;
    }
    assertType('0', $file->ftell());
    // This file is not empty.
    // call method that has side effects
    $file->fwrite('a');
    // the value of ftell may have changed
  }

}