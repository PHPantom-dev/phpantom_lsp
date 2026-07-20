@php
    /**
     * @var App\ViewModels\MembershipViewModel $model
     */
@endphp

<div class="alert {{ $model->isInDebt ? 'danger' : '' }}">
    @if ($model->isInDebt)
        <p>debt</p>
    @elseif (count($model->subscriptions) === 0 || !$model->lastSubscriptionAgreement)
        <p>none</p>
    @elseif($model->lastSubscriptionAgreement->state->isActive())
        <p>
            @switch($model->cancelError)
                @case (App\Enums\CancelSubscriptionError::EXPIRED)
                @break

                @case (App\Enums\CancelSubscriptionError::NONE)
                    <a href="{{ route('cancel.step-intro') }}">cancel</a>
                @break
            @endswitch
        </p>
    @else
        <p>fallback</p>
    @endif
</div>
