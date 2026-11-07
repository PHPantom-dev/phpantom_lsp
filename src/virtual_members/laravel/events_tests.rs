use super::*;

fn listener(event: &str, listener: &str, method: Option<&str>) -> ListenerRegistration {
    ListenerRegistration {
        event_fqn: event.to_string(),
        listener_fqn: listener.to_string(),
        method: method.map(str::to_string),
    }
}

fn observer(model: &str, observer: &str) -> ObserverRegistration {
    ObserverRegistration {
        model_fqn: model.to_string(),
        observer_fqn: observer.to_string(),
    }
}

#[test]
fn reads_event_listen_calls_in_every_listener_spelling() {
    let content = r#"<?php
namespace App\Providers;

use App\Events\OrderShipped;
use App\Events\OrderPlaced;
use App\Listeners\SendShipmentNotification;
use App\Listeners\AuditOrders;
use Illuminate\Support\Facades\Event;

class AppServiceProvider {
    public function boot(): void {
        Event::listen(OrderShipped::class, SendShipmentNotification::class);
        Event::listen([OrderShipped::class, OrderPlaced::class], [AuditOrders::class, 'record']);
        Event::listen(OrderPlaced::class, 'App\Listeners\Mailer@send');
        Event::listen(function (OrderShipped $event) {});
    }
}
"#;
    let scan = scan_event_registrations(content);
    assert_eq!(
        scan.listeners,
        vec![
            listener(
                "App\\Events\\OrderShipped",
                "App\\Listeners\\SendShipmentNotification",
                None
            ),
            listener(
                "App\\Events\\OrderShipped",
                "App\\Listeners\\AuditOrders",
                Some("record")
            ),
            listener(
                "App\\Events\\OrderPlaced",
                "App\\Listeners\\AuditOrders",
                Some("record")
            ),
            listener(
                "App\\Events\\OrderPlaced",
                "App\\Listeners\\Mailer",
                Some("send")
            ),
        ]
    );
}

#[test]
fn ignores_listen_on_a_class_that_is_not_the_event_facade() {
    let content = r#"<?php
namespace App;

class Socket {
    public function boot(): void {
        Server::listen(Request::class, Handler::class);
    }
}
"#;
    assert!(scan_event_registrations(content).is_empty());
}

#[test]
fn reads_listen_and_observers_properties() {
    let content = r#"<?php
namespace App\Providers;

use App\Events\Registered;
use App\Listeners\SendWelcome;
use App\Models\User;
use App\Models\Post;
use App\Observers\UserObserver;
use App\Observers\PostObserver;

class EventServiceProvider {
    protected $listen = [
        Registered::class => [SendWelcome::class, 'App\Listeners\Audit@handle'],
    ];

    protected $observers = [
        User::class => [UserObserver::class],
        Post::class => PostObserver::class,
    ];
}
"#;
    let scan = scan_event_registrations(content);
    assert_eq!(
        scan.listeners,
        vec![
            listener(
                "App\\Events\\Registered",
                "App\\Listeners\\SendWelcome",
                None
            ),
            listener(
                "App\\Events\\Registered",
                "App\\Listeners\\Audit",
                Some("handle")
            ),
        ]
    );
    assert_eq!(
        scan.observers,
        vec![
            observer("App\\Models\\User", "App\\Observers\\UserObserver"),
            observer("App\\Models\\Post", "App\\Observers\\PostObserver"),
        ]
    );
}

#[test]
fn reads_observe_calls_with_one_or_several_observers() {
    let content = r#"<?php
namespace App\Providers;

use App\Models\User;
use App\Observers\UserObserver;
use App\Observers\AuditObserver;

class AppServiceProvider {
    public function boot(): void {
        User::observe(UserObserver::class);
        User::observe([AuditObserver::class]);
    }
}
"#;
    let scan = scan_event_registrations(content);
    assert_eq!(
        scan.observers,
        vec![
            observer("App\\Models\\User", "App\\Observers\\UserObserver"),
            observer("App\\Models\\User", "App\\Observers\\AuditObserver"),
        ]
    );
}

#[test]
fn rebuild_merges_files_without_duplicates() {
    let mut index = LaravelEventIndex::default();
    let scan = EventScan {
        listeners: vec![listener("App\\E", "App\\L", None)],
        observers: vec![observer("App\\M", "App\\O")],
    };
    index
        .files
        .set_file("file:///a.php".to_string(), scan.clone());
    index.files.set_file("file:///b.php".to_string(), scan);
    index.rebuild();
    assert_eq!(index.listeners().len(), 1);
    assert_eq!(index.observers().len(), 1);
}
