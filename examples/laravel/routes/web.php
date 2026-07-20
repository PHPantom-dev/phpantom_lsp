<?php
use App\Http\Controllers\BakeryController;
use Illuminate\Support\Facades\Route;

Route::get('/', fn() => view('welcome'))->name('home');
Route::get('/bakeries', [BakeryController::class, 'index'])->name('bakeries.index');

Route::prefix('admin')->group(function () {
    Route::get('/users', fn() => view('admin.users.index'))->name('admin.users.index');
});

Route::prefix('bakeries')
    ->controller(BakeryController::class)
    ->group(function () {
        Route::get('{bakery}', 'show')->name('bakeries.show');
        Route::patch('{bakery}/cancel', 'cancel')->name('bakeries.cancel');
    });
