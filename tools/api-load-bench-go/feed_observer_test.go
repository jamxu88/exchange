package main

import "testing"

func TestApplyL2EventsBuildsExpectedBook(t *testing.T) {
	state := l2BookState{
		Initialized: true,
		Sequence:    4,
		Bids: map[uint64]uint64{
			100: 3,
		},
		Asks: map[uint64]uint64{
			101: 2,
		},
	}

	applyL2Events(&state, []BookEvent{
		{Kind: "level_updated", Side: "BUY", Price: 100, Quantity: 5},
		{Kind: "level_updated", Side: "SELL", Price: 101, Quantity: 0},
		{Kind: "level_updated", Side: "SELL", Price: 102, Quantity: 4},
		{Kind: "trade", Price: 102, Quantity: 1},
	}, 8)

	if state.Sequence != 8 {
		t.Fatalf("expected sequence 8, got %d", state.Sequence)
	}
	if state.Bids[100] != 5 {
		t.Fatalf("expected bid 100 quantity 5, got %d", state.Bids[100])
	}
	if _, ok := state.Asks[101]; ok {
		t.Fatalf("expected ask 101 to be removed")
	}
	if state.Asks[102] != 4 {
		t.Fatalf("expected ask 102 quantity 4, got %d", state.Asks[102])
	}
}

func TestApplyL3EventsAndAggregateMatchL2Shape(t *testing.T) {
	state := l3BookState{
		Initialized: true,
		Sequence:    10,
		Orders: map[string]l3OrderState{
			"bid-1": {Side: "BUY", Price: 100, Remaining: 3},
			"ask-1": {Side: "SELL", Price: 101, Remaining: 2},
		},
	}

	applyL3Events(&state, []wsL3Event{
		{Kind: "order_updated", OrderID: "bid-1", Side: "BUY", Price: 100, Remaining: 5},
		{Kind: "order_added", OrderID: "ask-2", Side: "SELL", Price: 101, Remaining: 4},
		{Kind: "trade", Quantity: 1, Price: 101},
		{Kind: "order_removed", OrderID: "ask-1"},
	}, 14)

	if state.Sequence != 14 {
		t.Fatalf("expected sequence 14, got %d", state.Sequence)
	}
	if len(state.Orders) != 2 {
		t.Fatalf("expected 2 active orders, got %d", len(state.Orders))
	}

	aggregate := aggregateL3Book(state)
	expected := l2BookState{
		Initialized: true,
		Sequence:    14,
		Bids: map[uint64]uint64{
			100: 5,
		},
		Asks: map[uint64]uint64{
			101: 4,
		},
	}

	if !equalL2Book(aggregate, expected) {
		t.Fatalf("expected aggregate %+v, got %+v", expected, aggregate)
	}
}
