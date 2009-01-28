%% File: adt.hrl

%%---------------------------------------------------------------------
%% Data Type: adt
%% where:
%%    type: An atom (default is undefined).
%%    ids: A list of integers (default is []).
%%    name: A string (default is undefined).
%%    extra: Dictionnary containing extra information.
%%        A {Key, Value} list (default is the empty list).
%%----------------------------------------------------------------------
-record(adt, {type, ids=[], name, extra=[]}).

