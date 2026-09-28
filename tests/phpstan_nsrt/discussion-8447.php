<?php

namespace Discussion8447;

use function PHPStan\Testing\assertType;

interface Quote
{
}

/**
 * @template TQuote of Quote
 */
interface QuoteRepository
{
	/** @return TQuote */
	public function create(): Quote;
}

/**
 * @template TQuoteRepository of QuoteRepository
 */
interface Lead
{
	/** @return TQuoteRepository */
	public function quoteRepository(): QuoteRepository;
}

/**
 * @template TLead of Lead
 * @template TQuote of Quote
 */
class Controller
{
	/**
	 * @template TQuoteRepository of QuoteRepository<TQuote>
	 * @param TLead<TQuoteRepository> $lead
	 * @return TQuote
	 */
	public function store(Lead $lead): Quote
	{
		assertType('TQuote of Discussion8447\Quote (class Discussion8447\Controller, argument)', $lead->quoteRepository()->create());
		return $lead->quoteRepository()->create();
	}
}
