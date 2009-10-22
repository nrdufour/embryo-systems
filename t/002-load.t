#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

test_matrix_element({FromState, [{Operation, ToState}|Rest]}) ->
	Message = io_lib:format("\t~-8s[~-10s] -> ~p", [Operation, FromState, ToState]),
	etap:is(adtm_util:new_state_after(Operation, FromState), ToState, Message),
	test_matrix_element({FromState, Rest});
test_matrix_element({_FromState, []}) ->
	ok.

test_matrix([Element|Rest]) ->
	test_matrix_element(Element),
	test_matrix(Rest);
test_matrix([]) ->
	ok.

main(_) ->
    etap:plan(unknown),

    etap_can:loaded_ok(adtm_util, "Module 'adtm_util' loaded"),

    Base = [
    	{none,      [   {create, alive}, {hibern, wrong_state}, {awake, wrong_state},
			{destroy, wrong_state}, {resur, wrong_state}, {purge, wrong_state} ]},
    	{alive,     [   {create, wrong_state}, {hibern, frozen}, {awake, wrong_state},
			{destroy, destroyed}, {resur, wrong_state}, {purge, wrong_state} ]},
    	{frozen,    [   {create, wrong_state}, {hibern, wrong_state}, {awake, alive},
			{destroy, wrong_state}, {resur, wrong_state}, {purge, wrong_state} ]},
    	{destroyed, [   {create, wrong_state}, {hibern, wrong_state}, {awake, wrong_state},
			{destroy, wrong_state}, {resur, alive}, {purge, none} ]}
    ],

    test_matrix(Base),

    %%etap:is(adtm_util:new_state_after(destroy, alive), destroyed, "Destroy/Alive -> Destroyed"),

    etap:end_tests(),
    ok.
