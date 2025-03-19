* Move to im crate for in-memory storage, move in direction of cloning it cheaply
* Get rid of EventHeight::try_into_i64 and provide sqlx::Encode impls
* Ensure instantiated contracts have set themselves as the admin.
* Keep track of outstanding messages for execution, and listeners notify when they've been executed. And how do we deal with that for failed transactions?
* Allow for a "compatible version comparison" within KolmeApp
* Configure bridge contracts with supported assets and reject others?
* Review handling of failed transaction processing and determine what we should do in different cases
