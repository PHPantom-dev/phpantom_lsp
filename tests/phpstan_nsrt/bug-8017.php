<?php

namespace Bug8017;

use function PHPStan\Testing\assertType;

function (): void {
	/** @var array{agency_name: non-empty-string, agent_fee: int, binder_number: null|non-empty-string, cancellation_type: null|non-empty-string, carrier_fee: int, carrier_inspection_fee: int, carrier_naic: null|non-empty-string, carrier_name: non-empty-string, clearinghouse: int, controlling_state: non-empty-string, coverage_limits: int, deductible: int, effective_date: non-empty-string, emp_a: int, endorsement_id: int, fire_marshall_tax: int, healthy_homes: int, inspection_fee: int, insured_name: non-empty-string, invoice_details_id: int, invoice_number: non-empty-string, lob: null|non-empty-string, mga_fee: int, operations_description: null|string, placement_reason: null|non-empty-string, policy_effective_date: non-empty-string, policy_expiration_date: non-empty-string, policy_number: non-empty-string, policy_status: int, policy_type: non-empty-string, premium: int, primary_risk_location_address_1: null|string, primary_risk_location_address_2: null|string, primary_risk_location_city: null|string, primary_risk_location_state: null|string, primary_risk_location_zip: null|string, return_premium: int, stamping_fee: int, state_tax: int, taxes_collected_by: int, transaction_date: non-empty-string, transaction_type: int, transaction_type_original: int, will_comply_fee: int, wind_pool_fee: int} $payload */
	$payload = [];
	assertType('int', $payload['premium']);

	$cancellationType = null !== $payload['cancellation_type'] ? 'yeah' : null;
	$lob = null !== $payload['lob'] ? 'yeah' : null;

	assertType('int', $payload['premium']);
};
