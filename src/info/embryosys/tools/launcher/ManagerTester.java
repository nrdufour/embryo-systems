/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.tools.launcher;

import info.embryosys.adt.manager.AdtManager;
import info.embryosys.adt.manager.impl.SimpleAdtManager;

/**
 * @author nrdufour
 * 
 */
public class ManagerTester {

	private AdtManager manager;
	
	/**
	 * 
	 */
	public ManagerTester() {
		this.manager = new SimpleAdtManager();
	}

	public void run() {

	}

	/**
	 * @param args
	 */
	public static void main(String[] args) {
		// TODO Auto-generated method stub

		ManagerTester tester = new ManagerTester();

		tester.run();
	}

}
