* Can I drop the event state entirely and keep it in the database?
* Get rid of EventHeight::try_into_i64 and provide sqlx::Encode impls
* Need to consolidate naming around messages from the contract and back to the contract. I made a mix of events, messages, actions, logs, and more in all of this.
* State storage: is the framework piece of the exec stream "execution state" or "framework state"?
* Processor produces its own event for "I have received sufficient listener signatures to approve an event"
* Ensure instantiated contracts have set themselves as the admin.
* Keep track of outstanding messages for execution, and listeners notify when they've been executed. And how do we deal with that for failed transactions?
* Allow for a "compatible version comparison" within KolmeApp
